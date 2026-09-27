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
          repository_scope TEXT,
          source_change_permission TEXT NOT NULL DEFAULT 'none'
            CHECK(source_change_permission IN ('none','isolated')),
          status TEXT NOT NULL,
          stop_reason TEXT,
          stalled_from TEXT,
          stall_reason TEXT,
          no_progress_turns INTEGER NOT NULL DEFAULT 0,
          failed_planning_turns INTEGER NOT NULL DEFAULT 0,
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          control_revision INTEGER NOT NULL DEFAULT 0,
          limit_revision INTEGER NOT NULL DEFAULT 0,
          allowed_targets TEXT NOT NULL,
          policy TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL
        );
        DROP INDEX IF EXISTS swarm_active_category;
        CREATE UNIQUE INDEX swarm_active_category
          ON swarm_runs(category_key)
          WHERE status IN ('planning','running','paused','stalled','draining','stopping');
        CREATE TABLE IF NOT EXISTS swarm_create_requests(
          request_scope TEXT NOT NULL,
          request_id TEXT NOT NULL,
          request_sha256 TEXT NOT NULL,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(request_scope,request_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_revision_requests(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          request_id TEXT NOT NULL,
          request_sha256 TEXT NOT NULL,
          result_json TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,request_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_stop_requests(
          request_scope TEXT NOT NULL,
          request_id TEXT NOT NULL,
          request_sha256 TEXT NOT NULL,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          result_json TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(request_scope,request_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_limit_events(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          request_id TEXT NOT NULL,
          expected_revision INTEGER NOT NULL,
          limit_revision INTEGER NOT NULL,
          old_max_workers INTEGER NOT NULL,
          new_max_workers INTEGER NOT NULL,
          request_max_workers INTEGER,
          request_backlog_max INTEGER,
          old_backlog_max INTEGER,
          new_backlog_max INTEGER,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,request_id),
          UNIQUE(run_id,limit_revision)
        );
        CREATE TABLE IF NOT EXISTS swarm_deadline_extensions(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          request_id TEXT NOT NULL,
          expected_deadline_at_ms INTEGER NOT NULL,
          additional_ms INTEGER NOT NULL,
          old_deadline_at_ms INTEGER NOT NULL,
          new_deadline_at_ms INTEGER NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,request_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_availability(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
          state TEXT NOT NULL CHECK(state IN ('eligible','blocked')),
          reason TEXT,
          eligible_targets TEXT NOT NULL,
          purpose TEXT NOT NULL,
          request_sha256 TEXT NOT NULL,
          snapshot_sha256 TEXT NOT NULL,
          observed_ms INTEGER NOT NULL,
          expires_ms INTEGER NOT NULL,
          wake_count INTEGER NOT NULL DEFAULT 0,
          updated_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_benefit_decisions(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          revision INTEGER NOT NULL,
          wave INTEGER NOT NULL,
          request_sha256 TEXT NOT NULL,
          decision TEXT NOT NULL CHECK(decision IN ('serial','parallel','blocked')),
          reason TEXT NOT NULL,
          max_parallel_workers INTEGER NOT NULL,
          job_ids TEXT NOT NULL,
          estimate_json TEXT NOT NULL,
          result_json TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,revision,wave)
        );
        CREATE TABLE IF NOT EXISTS swarm_benefit_attempt_outcomes(
          attempt_id TEXT PRIMARY KEY REFERENCES swarm_attempts(id) ON DELETE CASCADE,
          run_id TEXT NOT NULL,
          revision INTEGER NOT NULL,
          wave INTEGER NOT NULL,
          job_id TEXT NOT NULL,
          estimate_elapsed_ms INTEGER NOT NULL,
          estimate_usage_milli TEXT NOT NULL,
          actual_elapsed_ms INTEGER,
          actual_usage_milli TEXT,
          actual_source TEXT,
          observed_ms INTEGER,
          FOREIGN KEY(run_id,revision,wave)
            REFERENCES swarm_benefit_decisions(run_id,revision,wave)
        );
        CREATE INDEX IF NOT EXISTS swarm_benefit_outcomes_run
          ON swarm_benefit_attempt_outcomes(run_id,revision,wave);
        CREATE TABLE IF NOT EXISTS swarm_jobs(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          id TEXT NOT NULL,
          plan_revision INTEGER NOT NULL,
          title TEXT NOT NULL,
          acceptance TEXT NOT NULL,
          deps TEXT NOT NULL,
          resource_claims TEXT NOT NULL DEFAULT '[]',
          required_capabilities TEXT NOT NULL DEFAULT '[]',
          status TEXT NOT NULL,
          attempt_count INTEGER NOT NULL DEFAULT 0,
          deadline_at_ms INTEGER,
          stop_reason TEXT,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,id)
        );
        CREATE INDEX IF NOT EXISTS swarm_jobs_page ON swarm_jobs(run_id,id);
        CREATE INDEX IF NOT EXISTS swarm_jobs_status_page ON swarm_jobs(run_id,status,id);
        CREATE TABLE IF NOT EXISTS swarm_attempts(
          id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          revision INTEGER NOT NULL,
          token_sha256 TEXT NOT NULL,
          status TEXT NOT NULL,
          executor TEXT NOT NULL DEFAULT 'worker' CHECK(executor IN ('worker','director')),
          executor_run_id TEXT,
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
        CREATE TABLE IF NOT EXISTS swarm_operation_order(
          seq INTEGER PRIMARY KEY AUTOINCREMENT,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          kind TEXT NOT NULL CHECK(kind IN ('result','accept','revoke','stop')),
          created_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS swarm_operation_run ON swarm_operation_order(run_id,seq);
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
        CREATE TABLE IF NOT EXISTS swarm_resource_observations(
          run_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL REFERENCES swarm_attempts(id),
          resource TEXT NOT NULL,
          mode TEXT NOT NULL CHECK(mode IN ('read','write')),
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          PRIMARY KEY(attempt_id,resource)
        );
        CREATE INDEX IF NOT EXISTS swarm_observations_resource
          ON swarm_resource_observations(resource);
        CREATE TABLE IF NOT EXISTS swarm_resource_contamination(
          run_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL REFERENCES swarm_attempts(id),
          resource TEXT NOT NULL,
          peer_run_id TEXT NOT NULL,
          peer_job_id TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,job_id,attempt_id,resource,peer_run_id,peer_job_id)
        );
        CREATE INDEX IF NOT EXISTS swarm_contamination_attempt
          ON swarm_resource_contamination(run_id,job_id,attempt_id);
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
        CREATE TABLE IF NOT EXISTS swarm_integrations(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
          repo_root TEXT NOT NULL,
          base_commit TEXT NOT NULL,
          workspace_path TEXT NOT NULL,
          branch TEXT NOT NULL,
          current_commit TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_integrated_artifacts(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          artifact_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          commit_sha TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,artifact_id),
          FOREIGN KEY(run_id,artifact_id) REFERENCES swarm_artifacts(run_id,id)
        );
        CREATE TABLE IF NOT EXISTS swarm_verifications(
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          request_id TEXT NOT NULL,
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          commit_sha TEXT NOT NULL,
          verifier_sha256 TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('running','passed','failed','interrupted')),
          exit_code INTEGER,
          stdout TEXT NOT NULL DEFAULT '',
          stderr TEXT NOT NULL DEFAULT '',
          created_ms INTEGER NOT NULL,
          finished_ms INTEGER,
          UNIQUE(run_id,request_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_integration_intents(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
          artifact_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          prior_commit TEXT NOT NULL,
          expected_tree TEXT NOT NULL,
          artifact_sha256 TEXT NOT NULL,
          created_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_artifact_revocations(
          run_id TEXT NOT NULL,
          artifact_id TEXT NOT NULL,
          target_id TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,artifact_id,target_id),
          FOREIGN KEY(run_id,artifact_id) REFERENCES swarm_artifacts(run_id,id)
        );
        CREATE TABLE IF NOT EXISTS swarm_artifact_grants(
          run_id TEXT NOT NULL,
          artifact_id TEXT NOT NULL,
          target_id TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,artifact_id,target_id),
          FOREIGN KEY(run_id,artifact_id) REFERENCES swarm_artifacts(run_id,id)
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
        CREATE TABLE IF NOT EXISTS swarm_conflicts(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          conflict_id TEXT NOT NULL,
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          left_job_id TEXT NOT NULL,
          left_artifact_id TEXT NOT NULL,
          right_job_id TEXT NOT NULL,
          right_artifact_id TEXT NOT NULL,
          reason TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('open','resolved','unresolved')),
          outcome TEXT,
          reproduction_job_id TEXT,
          reproduction_artifact_id TEXT,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,conflict_id)
        );
        CREATE INDEX IF NOT EXISTS swarm_conflicts_by_status
          ON swarm_conflicts(run_id,status);
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
        CREATE TABLE IF NOT EXISTS swarm_partial_reports(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
          request_sha256 TEXT NOT NULL,
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          reason TEXT NOT NULL CHECK(length(reason)>0 AND length(reason)<=128),
          summary TEXT NOT NULL,
          limitations TEXT NOT NULL,
          created_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_completion_invalidations(
          run_id TEXT PRIMARY KEY REFERENCES swarm_completions(run_id),
          reason TEXT NOT NULL,
          resource TEXT NOT NULL,
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
          resolved_conflict_count_at_claim INTEGER NOT NULL DEFAULT 0,
          applied_count INTEGER NOT NULL DEFAULT 0,
          pending_review_count INTEGER NOT NULL DEFAULT 0,
          created_ms INTEGER NOT NULL,
          completed_ms INTEGER
        );
        CREATE TABLE IF NOT EXISTS swarm_director_owners(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
          generation INTEGER NOT NULL,
          token_sha256 TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('active','released')),
          created_ms INTEGER NOT NULL,
          renewed_ms INTEGER NOT NULL,
          lease_expires_ms INTEGER NOT NULL,
          overseer_run_id TEXT REFERENCES runs(id),
          supervised_launch INTEGER NOT NULL DEFAULT 0 CHECK(supervised_launch IN (0,1)),
          launch_phase TEXT CHECK(launch_phase IN ('reserved','linked','spawn_requested'))
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
          target_harness TEXT,
          target_profile_id TEXT,
          target_model TEXT,
          target_effort TEXT,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,request_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_admission_observations(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
          job_id TEXT NOT NULL,
          target_id TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('blocked','admitted')),
          reason TEXT,
          observed_ms INTEGER NOT NULL
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
          created_ms INTEGER NOT NULL,
          launch_phase TEXT CHECK(launch_phase IN ('reserved','linked','spawn_requested'))
        );
        CREATE TABLE IF NOT EXISTS swarm_stop_signals(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          overseer_run_id TEXT NOT NULL REFERENCES runs(id),
          attempts INTEGER NOT NULL,
          last_attempt_ms INTEGER NOT NULL,
          last_outcome TEXT NOT NULL CHECK(last_outcome IN ('requested','unconfirmed')),
          PRIMARY KEY(run_id,overseer_run_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_worker_liveness(
          attempt_id TEXT PRIMARY KEY REFERENCES swarm_attempts(id),
          state TEXT NOT NULL CHECK(state IN ('reachable','suspect','unknown')),
          unreachable_since_ms INTEGER,
          last_sample_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_effects(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          effect_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL REFERENCES swarm_attempts(id),
          revision INTEGER NOT NULL,
          operation_sha256 TEXT NOT NULL,
          outcome TEXT NOT NULL CHECK(outcome IN ('unknown','applied')),
          proof_artifact_id TEXT,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,effect_id),
          FOREIGN KEY(run_id,job_id) REFERENCES swarm_jobs(run_id,id)
        );
        CREATE INDEX IF NOT EXISTS swarm_effects_job ON swarm_effects(run_id,job_id,outcome);
        CREATE UNIQUE INDEX IF NOT EXISTS swarm_effects_operation
          ON swarm_effects(run_id,operation_sha256);
        "#,
    )?;
    // Older databases restricted partial reports to conflict/exhaustion. Keep
    // their reports while allowing a director to cite a fresh persisted
    // availability block by its exact reason.
    let partial_schema: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='swarm_partial_reports'",
        [],|row|row.get(0),
    )?;
    if partial_schema.contains("CHECK(reason IN") {
        let tx=conn.unchecked_transaction()?;
        tx.execute_batch(
            "CREATE TABLE swarm_partial_reports_next(
                run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
                request_sha256 TEXT NOT NULL,generation INTEGER NOT NULL,
                revision INTEGER NOT NULL,
                reason TEXT NOT NULL CHECK(length(reason)>0 AND length(reason)<=128),
                summary TEXT NOT NULL,limitations TEXT NOT NULL,
                created_ms INTEGER NOT NULL);
             INSERT INTO swarm_partial_reports_next
                SELECT * FROM swarm_partial_reports;
             DROP TABLE swarm_partial_reports;
             ALTER TABLE swarm_partial_reports_next RENAME TO swarm_partial_reports;",
        )?;
        tx.commit()?;
    }
    let has_stop_reason = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='stop_reason'")?
        .exists([])?;
    if !has_stop_reason {
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN stop_reason TEXT;")?;
    }
    let has_director_process = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_owners') WHERE name='overseer_run_id'")?
        .exists([])?;
    if !has_director_process {
        conn.execute_batch("ALTER TABLE swarm_director_owners ADD COLUMN overseer_run_id TEXT REFERENCES runs(id);")?;
    }
    let has_supervised_launch = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_owners') WHERE name='supervised_launch'")?
        .exists([])?;
    if !has_supervised_launch {
        conn.execute_batch("ALTER TABLE swarm_director_owners ADD COLUMN supervised_launch INTEGER NOT NULL DEFAULT 0 CHECK(supervised_launch IN (0,1));")?;
    }
    let has_launch_phase = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_owners') WHERE name='launch_phase'")?
        .exists([])?;
    if !has_launch_phase {
        conn.execute_batch("ALTER TABLE swarm_director_owners ADD COLUMN launch_phase TEXT CHECK(launch_phase IN ('reserved','linked','spawn_requested'));")?;
    }
    let has_worker_launch_phase = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_worker_launches') WHERE name='launch_phase'")?
        .exists([])?;
    if !has_worker_launch_phase {
        // An unlinked intent could not have spawned: task/run/intent linkage
        // commits before launch. Historical linked rows lack that proof.
        conn.execute_batch("ALTER TABLE swarm_worker_launches ADD COLUMN launch_phase TEXT CHECK(launch_phase IN ('reserved','linked','spawn_requested'));
            UPDATE swarm_worker_launches SET launch_phase='reserved' WHERE overseer_run_id IS NULL;")?;
    }
    let has_attempt_executor = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_attempts') WHERE name='executor'")?
        .exists([])?;
    if !has_attempt_executor {
        conn.execute_batch("ALTER TABLE swarm_attempts ADD COLUMN executor TEXT NOT NULL DEFAULT 'worker' CHECK(executor IN ('worker','director'));")?;
    }
    let has_executor_run_id = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_attempts') WHERE name='executor_run_id'")?
        .exists([])?;
    if !has_executor_run_id {
        conn.execute_batch("ALTER TABLE swarm_attempts ADD COLUMN executor_run_id TEXT;")?;
    }
    let has_source_change_permission = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='source_change_permission'",
        )?
        .exists([])?;
    if !has_source_change_permission {
        // An old run has no durable grant to change source, even if its objective
        // happens to describe implementation work.
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN source_change_permission TEXT NOT NULL DEFAULT 'none' CHECK(source_change_permission IN ('none','isolated'));")?;
    }
    let has_repository_scope = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='repository_scope'")?
        .exists([])?;
    if !has_repository_scope {
        // Legacy fixture runs have no recorded repository authorization. Do not
        // infer a grant from their objective or from an existing process path.
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN repository_scope TEXT;")?;
    }
    let has_control_revision = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='control_revision'")?
        .exists([])?;
    if !has_control_revision {
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN control_revision INTEGER NOT NULL DEFAULT 0;")?;
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
    let has_limit_revision = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='limit_revision'")?
        .exists([])?;
    if !has_limit_revision {
        conn.execute_batch(
            "ALTER TABLE swarm_runs ADD COLUMN limit_revision INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    for column in ["request_max_workers", "request_backlog_max", "old_backlog_max", "new_backlog_max"] {
        let exists = conn.prepare("SELECT 1 FROM pragma_table_info('swarm_limit_events') WHERE name=?1")?
            .exists([column])?;
        if !exists {
            conn.execute_batch(&format!("ALTER TABLE swarm_limit_events ADD COLUMN {column} INTEGER;"))?;
        }
    }
    let has_no_progress_turns = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='no_progress_turns'")?
        .exists([])?;
    if !has_no_progress_turns {
        conn.execute_batch(
            "ALTER TABLE swarm_runs ADD COLUMN no_progress_turns INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    let has_availability_request_sha256 = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('swarm_availability') WHERE name='request_sha256'",
        )?
        .exists([])?;
    if !has_availability_request_sha256 {
        // Old draft observations have no assessment identity. Keep them blocked from
        // being reinterpreted as recovery under changed requirements.
        conn.execute_batch(
            "ALTER TABLE swarm_availability ADD COLUMN request_sha256 TEXT NOT NULL DEFAULT '';
             UPDATE swarm_availability SET state='blocked',reason='assessment_unknown',eligible_targets='[]';",
        )?;
    }
    let has_availability_snapshot_sha256 = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_availability') WHERE name='snapshot_sha256'")?
        .exists([])?;
    if !has_availability_snapshot_sha256 {
        // A saved observation without its source snapshot cannot authorize a
        // later admission. A fresh observation restores eligibility.
        conn.execute_batch(
            "ALTER TABLE swarm_availability ADD COLUMN snapshot_sha256 TEXT NOT NULL DEFAULT '';
             UPDATE swarm_availability SET state='blocked',reason='snapshot_unknown',eligible_targets='[]'
             WHERE state='eligible';",
        )?;
    }
    let has_failed_planning_turns = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='failed_planning_turns'",
        )?
        .exists([])?;
    if !has_failed_planning_turns {
        conn.execute_batch(
            "ALTER TABLE swarm_runs ADD COLUMN failed_planning_turns INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    let has_decision_snapshot = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_turns') WHERE name='accepted_decision_id_at_claim'")?
        .exists([])?;
    if !has_decision_snapshot {
        conn.execute_batch(
            "ALTER TABLE swarm_director_turns ADD COLUMN accepted_decision_id_at_claim INTEGER NOT NULL DEFAULT 0;
             UPDATE swarm_director_turns SET accepted_decision_id_at_claim=(
               SELECT COALESCE(MAX(id),0) FROM swarm_decisions d
               WHERE d.run_id=swarm_director_turns.run_id AND d.decision='accept'
             ) WHERE status='active';",
        )?;
    }
    let has_conflict_snapshot = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_turns') WHERE name='resolved_conflict_count_at_claim'")?
        .exists([])?;
    if !has_conflict_snapshot {
        conn.execute_batch(
            "ALTER TABLE swarm_director_turns ADD COLUMN resolved_conflict_count_at_claim INTEGER NOT NULL DEFAULT 0;
             UPDATE swarm_director_turns SET resolved_conflict_count_at_claim=(
               SELECT COUNT(*) FROM swarm_conflicts c
               WHERE c.run_id=swarm_director_turns.run_id AND c.status='resolved'
             ) WHERE status='active';",
        )?;
    }
    let has_reviewed_message_seq = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('swarm_decisions') WHERE name='reviewed_message_seq'",
        )?
        .exists([])?;
    if !has_reviewed_message_seq {
        conn.execute_batch("ALTER TABLE swarm_decisions ADD COLUMN reviewed_message_seq INTEGER NOT NULL DEFAULT 0;")?;
    }
    let has_job_deadline = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_jobs') WHERE name='deadline_at_ms'")?
        .exists([])?;
    if !has_job_deadline {
        conn.execute_batch("ALTER TABLE swarm_jobs ADD COLUMN deadline_at_ms INTEGER;")?;
    }
    let has_job_stop_reason = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_jobs') WHERE name='stop_reason'")?
        .exists([])?;
    if !has_job_stop_reason {
        conn.execute_batch("ALTER TABLE swarm_jobs ADD COLUMN stop_reason TEXT;")?;
    }
    let has_job_resource_claims = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_jobs') WHERE name='resource_claims'")?
        .exists([])?;
    if !has_job_resource_claims {
        conn.execute_batch(
            "ALTER TABLE swarm_jobs ADD COLUMN resource_claims TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    let has_job_required_capabilities = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_jobs') WHERE name='required_capabilities'")?
        .exists([])?;
    if !has_job_required_capabilities {
        conn.execute_batch(
            "ALTER TABLE swarm_jobs ADD COLUMN required_capabilities TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    let has_admitted_harness = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_admissions') WHERE name='target_harness'")?
        .exists([])?;
    if !has_admitted_harness {
        // Historical rows lack a trustworthy route-to-harness binding. They
        // cannot authorize a new worker process until reconciled.
        conn.execute_batch("ALTER TABLE swarm_admissions ADD COLUMN target_harness TEXT;")?;
    }
    for column in ["target_profile_id", "target_model", "target_effort"] {
        let exists = conn.prepare("SELECT 1 FROM pragma_table_info('swarm_admissions') WHERE name=?1")?
            .exists([column])?;
        if !exists {
            conn.execute_batch(&format!("ALTER TABLE swarm_admissions ADD COLUMN {column} TEXT;"))?;
        }
    }
    let has_turn_applied_count = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_turns') WHERE name='applied_count'")?
        .exists([])?;
    if !has_turn_applied_count {
        conn.execute_batch(
            "ALTER TABLE swarm_director_turns ADD COLUMN applied_count INTEGER NOT NULL DEFAULT 0;
             UPDATE swarm_director_turns SET applied_count=(
               SELECT COUNT(*) FROM swarm_director_turn_messages WHERE turn_id=swarm_director_turns.id
             ) WHERE status='complete';",
        )?;
    }
    let has_turn_pending_count = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_turns') WHERE name='pending_review_count'")?
        .exists([])?;
    if !has_turn_pending_count {
        conn.execute_batch(
            "ALTER TABLE swarm_director_turns ADD COLUMN pending_review_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_partial_reports_survive_availability_reason_migration() {
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO swarm_runs(id,category,category_key,objective,status,
                generation,revision,allowed_targets,policy,created_ms,updated_ms)
             VALUES('old-run','Audit','audit','Inspect','stopped',1,1,'[]','{}',1,1);
             DROP TABLE swarm_partial_reports;
             CREATE TABLE swarm_partial_reports(
                run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id) ON DELETE CASCADE,
                request_sha256 TEXT NOT NULL,generation INTEGER NOT NULL,
                revision INTEGER NOT NULL,
                reason TEXT NOT NULL CHECK(reason IN ('unresolved_conflict','attempts_exhausted')),
                summary TEXT NOT NULL,limitations TEXT NOT NULL,
                created_ms INTEGER NOT NULL);
             INSERT INTO swarm_partial_reports VALUES(
                'old-run','digest',1,1,'unresolved_conflict','Disputed result',
                'No reproduction',2);",
        ).unwrap();
        migrate(&conn).unwrap();
        let saved:(String,String)=conn.query_row(
            "SELECT reason,summary FROM swarm_partial_reports WHERE run_id='old-run'",
            [],|r|Ok((r.get(0)?,r.get(1)?)),
        ).unwrap();
        assert_eq!(saved,("unresolved_conflict".into(),"Disputed result".into()));
        conn.execute("INSERT INTO swarm_partial_reports VALUES(
            'new-run','digest',1,1,'allowed_target_missing','Unavailable','Unprobed',3)",[])
            .unwrap_err(); // the parent run must still exist
        conn.execute_batch(
            "INSERT INTO swarm_runs(id,category,category_key,objective,status,
                generation,revision,allowed_targets,policy,created_ms,updated_ms)
             VALUES('new-run','Audit 2','audit-2','Inspect','stopped',1,1,'[]','{}',1,1);")
            .unwrap();
        conn.execute("INSERT INTO swarm_partial_reports VALUES(
            'new-run','digest',1,1,'allowed_target_missing','Unavailable','Unprobed',3)",[])
            .unwrap();
        migrate(&conn).unwrap();
        let count:i64=conn.query_row("SELECT COUNT(*) FROM swarm_partial_reports",[],|r|r.get(0)).unwrap();
        assert_eq!(count,2);
    }

    #[test]
    fn existing_registered_attempts_remain_worker_attempts_after_upgrade() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE swarm_attempts(
            id TEXT PRIMARY KEY,run_id TEXT NOT NULL,job_id TEXT NOT NULL,
            revision INTEGER NOT NULL,token_sha256 TEXT NOT NULL,status TEXT NOT NULL,
            created_ms INTEGER NOT NULL);
            INSERT INTO swarm_attempts VALUES('old-attempt','old-run','old-job',1,'digest','registered',1);")
            .unwrap();
        migrate(&conn).unwrap();
        let (executor,status): (String,String) = conn.query_row(
            "SELECT executor,status FROM swarm_attempts WHERE id='old-attempt'",
            [],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!((executor.as_str(),status.as_str()),("worker","registered"));
        let executor_run_id: Option<String> = conn.query_row(
            "SELECT executor_run_id FROM swarm_attempts WHERE id='old-attempt'",
            [],|r|r.get(0)).unwrap();
        assert!(executor_run_id.is_none());
        migrate(&conn).unwrap();
    }

    #[test]
    fn old_admission_has_no_launchable_harness_binding_after_migration() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE swarm_admissions(
            run_id TEXT NOT NULL,request_id TEXT NOT NULL,request_sha256 TEXT NOT NULL,
            job_id TEXT NOT NULL,attempt_id TEXT NOT NULL,target_id TEXT NOT NULL,
            created_ms INTEGER NOT NULL,PRIMARY KEY(run_id,request_id));
            INSERT INTO swarm_admissions VALUES('run','request','hash','job','attempt','target',1);")
            .unwrap();
        migrate(&conn).unwrap();
        let binding: (Option<String>,Option<String>,Option<String>,Option<String>) = conn.query_row(
            "SELECT target_harness,target_profile_id,target_model,target_effort
             FROM swarm_admissions WHERE request_id='request'",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).unwrap();
        assert_eq!(binding,(None,None,None,None));
        migrate(&conn).unwrap();
    }

    #[test]
    fn old_worker_intent_migrates_only_unlinked_rows_to_proven_no_spawn() {
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE swarm_worker_launches(
            attempt_id TEXT PRIMARY KEY,run_id TEXT NOT NULL,job_id TEXT NOT NULL,
            request_sha256 TEXT NOT NULL,overseer_run_id TEXT UNIQUE,created_ms INTEGER NOT NULL);
            INSERT INTO swarm_worker_launches VALUES('pending','run','job','digest',NULL,1);
            INSERT INTO swarm_worker_launches VALUES('linked','run','job','digest','overseer-run',1);").unwrap();
        migrate(&conn).unwrap();
        let pending: Option<String>=conn.query_row(
            "SELECT launch_phase FROM swarm_worker_launches WHERE attempt_id='pending'",
            [],|r|r.get(0)).unwrap();
        let linked: Option<String>=conn.query_row(
            "SELECT launch_phase FROM swarm_worker_launches WHERE attempt_id='linked'",
            [],|r|r.get(0)).unwrap();
        assert_eq!(pending.as_deref(),Some("reserved"));
        assert_eq!(linked,None);
        migrate(&conn).unwrap();
    }

    #[test]
    fn old_director_owner_keeps_its_reservation_when_process_link_is_added() {
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE swarm_director_owners(
            run_id TEXT PRIMARY KEY,generation INTEGER NOT NULL,token_sha256 TEXT NOT NULL,
            status TEXT NOT NULL,created_ms INTEGER NOT NULL,renewed_ms INTEGER NOT NULL,
            lease_expires_ms INTEGER NOT NULL);
            INSERT INTO swarm_director_owners VALUES('old-run',1,'digest','active',1,2,30000);").unwrap();
        migrate(&conn).unwrap();
        let owner: (i64,String,Option<String>,i64,Option<String>)=conn.query_row(
            "SELECT generation,status,overseer_run_id,supervised_launch,launch_phase FROM swarm_director_owners WHERE run_id='old-run'",
            [],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
        assert_eq!(owner,(1,"active".into(),None,0,None));
        migrate(&conn).unwrap();
    }

    #[test]
    fn old_swarm_runs_do_not_inherit_source_change_permission() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE swarm_runs(
                id TEXT PRIMARY KEY,category TEXT NOT NULL,category_key TEXT NOT NULL,
                objective TEXT NOT NULL,status TEXT NOT NULL,generation INTEGER NOT NULL,
                revision INTEGER NOT NULL,allowed_targets TEXT NOT NULL,policy TEXT NOT NULL,
                created_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL);
             INSERT INTO swarm_runs VALUES('old-run','Audit','audit','Inspect only',
                'running',1,1,'[]','{}',1,1);",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let permission: String = conn.query_row(
            "SELECT source_change_permission FROM swarm_runs WHERE id='old-run'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(permission, "none");
        let repository_scope: Option<String> = conn.query_row(
            "SELECT repository_scope FROM swarm_runs WHERE id='old-run'",
            [], |row| row.get(0),
        ).unwrap();
        assert!(repository_scope.is_none(), "migration must not invent an approved repository");
        let limit_revision: i64 = conn.query_row(
            "SELECT limit_revision FROM swarm_runs WHERE id='old-run'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(limit_revision, 0);
        let control_revision: i64 = conn.query_row(
            "SELECT control_revision FROM swarm_runs WHERE id='old-run'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(control_revision, 0);
        migrate(&conn).unwrap();
    }

    #[test]
    fn old_availability_rows_fail_closed_during_migration() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE swarm_availability(
                run_id TEXT PRIMARY KEY,state TEXT NOT NULL,reason TEXT,
                eligible_targets TEXT NOT NULL,purpose TEXT NOT NULL,
                observed_ms INTEGER NOT NULL,expires_ms INTEGER NOT NULL,
                wake_count INTEGER NOT NULL,updated_ms INTEGER NOT NULL);
             INSERT INTO swarm_availability VALUES(
                'old-run','eligible',NULL,'[\"route-a\"]','worker',1,2,0,1);",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let row: (String, String, String, String) = conn
            .query_row(
                "SELECT state,reason,eligible_targets,request_sha256
                 FROM swarm_availability WHERE run_id='old-run'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "blocked".into(),
                "assessment_unknown".into(),
                "[]".into(),
                "".into()
            )
        );
        migrate(&conn).unwrap();
    }

    #[test]
    fn availability_without_snapshot_identity_fails_closed_during_migration() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE swarm_availability(
                run_id TEXT PRIMARY KEY,state TEXT NOT NULL,reason TEXT,
                eligible_targets TEXT NOT NULL,purpose TEXT NOT NULL,
                request_sha256 TEXT NOT NULL,observed_ms INTEGER NOT NULL,
                expires_ms INTEGER NOT NULL,wake_count INTEGER NOT NULL,
                updated_ms INTEGER NOT NULL);
             INSERT INTO swarm_availability VALUES(
                'old-run','eligible',NULL,'[\"route-a\"]','worker','assessment',1,2,0,1);",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let row: (String, String, String, String) = conn
            .query_row(
                "SELECT state,reason,eligible_targets,snapshot_sha256
                 FROM swarm_availability WHERE run_id='old-run'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "blocked".into(),
                "snapshot_unknown".into(),
                "[]".into(),
                "".into()
            )
        );
        migrate(&conn).unwrap();
    }

    #[test]
    fn old_completed_director_turns_keep_their_duplicate_receipt_count() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE swarm_director_turns(
                id TEXT PRIMARY KEY,run_id TEXT NOT NULL,generation INTEGER NOT NULL,
                revision INTEGER NOT NULL,token_sha256 TEXT NOT NULL,status TEXT NOT NULL,
                accepted_decision_id_at_claim INTEGER NOT NULL DEFAULT 0,
                created_ms INTEGER NOT NULL,completed_ms INTEGER);
             CREATE TABLE swarm_director_turn_messages(
                turn_id TEXT NOT NULL,seq INTEGER NOT NULL,PRIMARY KEY(turn_id,seq));
             INSERT INTO swarm_director_turns VALUES('old-turn','run',1,1,'hash','complete',0,1,2);
             INSERT INTO swarm_director_turn_messages VALUES('old-turn',1),('old-turn',2);",
        ).unwrap();
        migrate(&conn).unwrap();
        let counts: (i64, i64) = conn.query_row(
            "SELECT applied_count,pending_review_count FROM swarm_director_turns WHERE id='old-turn'",
            [], |row| Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!(counts,(2,0));
        migrate(&conn).unwrap();
    }

    #[test]
    fn active_legacy_turn_does_not_gain_false_progress_on_upgrade() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE swarm_director_turns(
                id TEXT PRIMARY KEY,run_id TEXT NOT NULL,generation INTEGER NOT NULL,
                revision INTEGER NOT NULL,token_sha256 TEXT NOT NULL,status TEXT NOT NULL,
                applied_count INTEGER NOT NULL DEFAULT 0,
                pending_review_count INTEGER NOT NULL DEFAULT 0,
                created_ms INTEGER NOT NULL,completed_ms INTEGER);
             CREATE TABLE swarm_conflicts(run_id TEXT NOT NULL,status TEXT NOT NULL);
             CREATE TABLE swarm_decisions(
               id INTEGER PRIMARY KEY,run_id TEXT NOT NULL,decision TEXT NOT NULL,
               reviewed_message_seq INTEGER NOT NULL DEFAULT 0);
             INSERT INTO swarm_director_turns VALUES('active-turn','run',1,1,'hash','active',0,0,1,NULL);
             INSERT INTO swarm_conflicts VALUES('run','resolved');
             INSERT INTO swarm_decisions VALUES(1,'run','accept',0);",
        ).unwrap();
        migrate(&conn).unwrap();
        let at_claim: (i64,i64) = conn.query_row(
            "SELECT resolved_conflict_count_at_claim,accepted_decision_id_at_claim
             FROM swarm_director_turns WHERE id='active-turn'",
            [], |row| Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!(at_claim,(1,1),"pre-upgrade decisions are not new turn progress");
        migrate(&conn).unwrap();
    }

    #[test]
    fn existing_jobs_gain_empty_capability_requirements_without_losing_the_plan() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE swarm_jobs(
                run_id TEXT NOT NULL,id TEXT NOT NULL,plan_revision INTEGER NOT NULL,
                title TEXT NOT NULL,acceptance TEXT NOT NULL,deps TEXT NOT NULL,
                resource_claims TEXT NOT NULL DEFAULT '[]',status TEXT NOT NULL,
                attempt_count INTEGER NOT NULL DEFAULT 0,deadline_at_ms INTEGER,
                stop_reason TEXT,created_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL,
                PRIMARY KEY(run_id,id));
             INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,
                resource_claims,status,created_ms,updated_ms)
             VALUES('old-run','job',2,'Inspect','Evidence','[]','[]','ready',1,1);",
        ).unwrap();
        migrate(&conn).unwrap();
        let row: (String, String, i64) = conn.query_row(
            "SELECT required_capabilities,status,plan_revision FROM swarm_jobs
             WHERE run_id='old-run' AND id='job'", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).unwrap();
        assert_eq!(row, ("[]".into(),"ready".into(),2));
        migrate(&conn).unwrap();
    }
}
