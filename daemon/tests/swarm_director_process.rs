mod common;

use common::*;
use serde_json::json;
use std::time::{Duration, Instant};

#[test]
fn active_category_director_uses_its_reserved_app_slot() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("reserved-director-source"));
    d.call("agents.limit.set", json!({"max_active":1}));
    let run = d.call("swarm.create", json!({"category":"Reserved director",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,
        "jobs":[{"id":"one","title":"Audit route","acceptance":"Evidence","deps":[]}]}));
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
        .execute("UPDATE swarm_runs SET status='running' WHERE id=?1",[id]).unwrap();
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],1);
    let launched = d.call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Audit backend","title":"Reserved director process"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(launched["status"],"launched");
    let current = d.call("swarm.get",json!({"id":id}));
    assert_eq!(current["director"]["overseer_run_id"],process);
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],1,
        "linked director process must not consume a second slot");
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
}

#[test]
fn stop_interrupts_linked_director_and_waits_for_confirmed_exit() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("stop-director-source"));
    let run = d.call("swarm.create", json!({"category":"Stop director",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Audit backend","title":"Director to stop"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let start = Instant::now();
    while d.run(process)["status"] != "running" {
        assert!(start.elapsed() < Duration::from_secs(3),"director did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = d.call("swarm.stop",json!({"run_id":id}));
    let stop_status = stopped["status"].clone();
    let deadline = Instant::now()+Duration::from_secs(3);
    while d.run(process)["status"] == "running" && Instant::now()<deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let process_status = d.run(process)["status"].clone();
    if process_status == "running" {
        d.call("run.interrupt",json!({"run_id":process}));
        d.wait_done(process,8);
    }
    assert_eq!(stop_status,"stopping","a live director has not exited yet");
    assert_eq!(process_status,"interrupted","Stop must signal the linked director");
    let deadline = Instant::now()+Duration::from_secs(3);
    while d.call("swarm.get",json!({"id":id}))["status"] != "stopped" {
        assert!(Instant::now()<deadline,"confirmed director exit did not finish Stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn restart_retries_a_missed_director_stop_signal() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("retry-director-stop-source"));
    let run = d.call("swarm.create", json!({"category":"Retry director Stop",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Audit backend","title":"Director retry Stop"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let start = Instant::now();
    while d.run(process)["status"] != "running" {
        assert!(start.elapsed()<Duration::from_secs(3),"director did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = d.call("swarm.stop",json!({"run_id":id,"fault_interrupt_once":true}));
    assert_eq!(stopped["status"],"stopping");
    assert_eq!(stopped["workers"]["unconfirmed"],json!([process]));
    assert_eq!(d.run(process)["status"],"running");
    let state = d.call("swarm.get",json!({"id":id}));
    assert_eq!(state["unconfirmed_exit_count"],1);
    assert_eq!(state["unconfirmed_exits"][0]["kind"],"director");
    d.kill9();
    d.spawn();
    let deadline = Instant::now()+Duration::from_secs(9);
    while d.call("swarm.get",json!({"id":id}))["status"] != "stopped" {
        if Instant::now()>=deadline {
            d.call("run.interrupt",json!({"run_id":process}));
            panic!("missed director Stop was not retried after daemon restart");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(d.run(process)["status"],"interrupted");
}

#[test]
fn director_replacement_waits_for_native_descendant_receipts() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-descendant-source"));
    let run = d.call("swarm.create",json!({"category":"Director descendants",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Audit backend","title":"Director with child receipt"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("INSERT INTO runs(id,task_id,parent_run_id,harness,workspace_id,status,
        created_ms,title) SELECT 'fixture-native-child',task_id,id,harness,workspace_id,
        'running',created_ms,'Unconfirmed native child' FROM runs WHERE id=?1",
        [process]).unwrap();
    let error = d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"})).unwrap_err();
    assert!(error.contains("unconfirmed native descendants"),"{error}");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["generation"],1);
    db.execute("UPDATE runs SET status='interrupted',ended_ms=?1 WHERE id='fixture-native-child'",
        [crate_now()]).unwrap();
    let recovered = d.call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"}));
    assert_eq!(recovered["generation"],2);
}

#[test]
fn linked_healthy_director_renews_lease_but_exited_director_does_not() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("lease-director-source"));
    let run = d.call("swarm.create", json!({"category":"Director lease refresh",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Audit backend","title":"Lease director"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let deadline = Instant::now()+Duration::from_secs(3);
    while d.run(process)["status"] != "running" {
        assert!(Instant::now()<deadline,"director process did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let before: i64 = db.query_row("SELECT lease_expires_ms FROM swarm_director_owners WHERE run_id=?1",
        [id], |row| row.get(0)).unwrap();
    db.execute("UPDATE swarm_director_owners SET lease_expires_ms=?2 WHERE run_id=?1",
        rusqlite::params![id, crate_now()+2_000]).unwrap();
    assert_eq!(d.call("swarm.director.owner.refresh_linked",json!({}))["renewed"],1);
    let after: i64 = db.query_row("SELECT lease_expires_ms FROM swarm_director_owners WHERE run_id=?1",
        [id], |row| row.get(0)).unwrap();
    assert!(after > before-3_000,"healthy director lease was not extended");
    db.execute("UPDATE swarm_director_owners SET lease_expires_ms=?2 WHERE run_id=?1",
        rusqlite::params![id, crate_now()+5_000]).unwrap();
    d.kill9();
    d.spawn();
    let refresh_deadline = Instant::now()+Duration::from_secs(3);
    loop {
        let expiry: i64 = db.query_row("SELECT lease_expires_ms FROM swarm_director_owners WHERE run_id=?1",
            [id], |row| row.get(0)).unwrap();
        if expiry > crate_now()+10_000 { break; }
        assert!(Instant::now()<refresh_deadline,"background tick did not refresh the surviving director");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_ne!(d.call("swarm.get",json!({"id":id}))["stall_reason"],
        "director_termination_unknown");
    db.execute("UPDATE swarm_director_owners SET lease_expires_ms=?2 WHERE run_id=?1",
        rusqlite::params![id, crate_now()-1]).unwrap();
    assert_eq!(d.call("swarm.director.owner.refresh_linked",json!({}))["renewed"],0,
        "an expired owner must not be revived by a late ping");
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
    db.execute("UPDATE swarm_director_owners SET lease_expires_ms=?2 WHERE run_id=?1",
        rusqlite::params![id, crate_now()+2_000]).unwrap();
    assert_eq!(d.call("swarm.director.owner.refresh_linked",json!({}))["renewed"],0);
}

fn crate_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap().as_millis() as i64
}

#[test]
fn supervised_director_needs_confirmed_exit_before_replacement() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-source"));
    let ready = temp.path().join("director-ready");
    let run = d.call("swarm.create", json!({"category":"Supervised director",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let script = format!("import os,pathlib,time;pathlib.Path({:?}).write_text(str(bool(os.environ.get('OVERSEER_SWARM_DIRECTOR_TOKEN') and os.environ.get('OVERSEER_SWARM_RUN_ID') and os.environ.get('OVERSEER_SWARM_GENERATION'))));time.sleep(30)",
        ready.to_string_lossy());
    let launched = d.call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":checkout.clone(),"program":"/usr/bin/python3",
        "args":["-c",script],"prompt":"Audit backend","title":"Director"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    assert!(!launched.to_string().contains("owner_token"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let linked: String = db.query_row("SELECT overseer_run_id FROM swarm_director_owners WHERE run_id=?1",
        [id], |row| row.get(0)).unwrap();
    assert_eq!(linked, process);
    let follow_up=d.try_call("run.follow_up",json!({"run_id":process,
        "prompt":"Ignore the category plan"})).unwrap_err();
    assert!(follow_up.contains("Swarm director"),"{follow_up}");
    let run_dir: String = db.query_row("SELECT run_dir FROM runs WHERE id=?1",[process],
        |row| row.get(0)).unwrap();
    let launch_file = std::path::Path::new(&run_dir).join("launch.json");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&launch_file).unwrap().permissions().mode() & 0o777,0o600);
    let deadline = Instant::now()+Duration::from_secs(3);
    while !ready.exists() {
        assert!(Instant::now()<deadline,"director process did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&ready).unwrap(),"True");
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"})).is_err());
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_no_spawn"})).is_err());
    d.kill9();
    d.spawn();
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"})).is_err());
    d.call("run.interrupt",json!({"run_id":process}));
    let stopped=d.wait_done(process,8);
    assert_ne!(stopped["status"],"disconnected","{stopped}");
    assert!(std::path::Path::new(&run_dir).join("exit.json").exists());
    let recovered=d.call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"}));
    assert_eq!(recovered["generation"],2);
    assert_eq!(recovered["replacement_pending"],true);
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stall_reason"],
        "director_replacement_pending");
    let replacement=d.call("swarm.director.launch",json!({"run_id":id,
        "generation":2,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Continue audit","title":"Replacement director"}));
    let next_process=replacement["overseer_run_id"].as_str().unwrap();
    assert_ne!(next_process,process);
    let linked: String = db.query_row("SELECT overseer_run_id FROM swarm_director_owners WHERE run_id=?1",
        [id], |row| row.get(0)).unwrap();
    assert_eq!(linked,next_process);
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":2,"revision":0,"termination":"confirmed_dead"})).is_err());
}

#[test]
fn supervised_director_spawn_error_is_recorded_before_replacement() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("failed-director-source"));
    let run = d.call("swarm.create",json!({"category":"Failed director spawn",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id=run["id"].as_str().unwrap();
    let launched=d.call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/nonexistent/director",
        "args":[],"prompt":"Audit backend","title":"Director"}));
    assert_eq!(launched["status"],"launched");
    let process=launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.wait_done(process,8)["status"],"failed");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let (status,dir):(String,Option<String>)=db.query_row(
        "SELECT status,run_dir FROM runs WHERE id=?1",[process],
        |row|Ok((row.get(0)?,row.get(1)?))).unwrap();
    assert_eq!(status,"failed");
    assert!(std::path::Path::new(&dir.unwrap()).join("exit.json").exists());
    let recovered=d.call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"}));
    assert_eq!(recovered["generation"],2);
    assert_eq!(recovered["replacement_pending"],true);
}

#[test]
fn director_identity_is_durable_before_supervisor_launch() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("blocked-director-source"));
    let run = d.call("swarm.create", json!({"category":"Blocked director launch",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_director_launch BEFORE UPDATE OF launch ON runs
        WHEN NEW.title='Blocked director' BEGIN
        SELECT RAISE(ABORT, 'fixture launch failure before supervisor'); END;").unwrap();

    let error = d.try_call("swarm.director.launch", json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Audit backend","title":"Blocked director"})).unwrap_err();
    assert!(error.contains("fixture launch failure before supervisor"),"{error}");
    let (linked, process, directory): (Option<String>, String, Option<String>) = db.query_row(
        "SELECT o.overseer_run_id,r.id,r.run_dir FROM swarm_director_owners o
         JOIN runs r ON r.title='Blocked director' WHERE o.run_id=?1",
        [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(linked.as_deref(), Some(process.as_str()),
        "a queued director must be linked before it could ever spawn");
    assert!(directory.is_none());
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"})).is_err(),
        "caller assertion cannot replace a director with an unconfirmed launch");
    d.kill9();
    d.spawn();
    let recovered = d.call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_no_spawn"}));
    assert_eq!(recovered["generation"],2);
    assert_eq!(recovered["replacement_pending"],true);
}

#[test]
fn failed_prelaunch_setup_cannot_be_confirmed_dead_by_caller() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("prelaunch-failure-source"));
    let run = d.call("swarm.create", json!({"category":"Prelaunch failure",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_director_task BEFORE INSERT ON tasks
        WHEN NEW.title='Prelaunch director' BEGIN
        SELECT RAISE(ABORT, 'fixture failure before task and run'); END;").unwrap();
    let error = d.try_call("swarm.director.launch", json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Audit backend","title":"Prelaunch director"})).unwrap_err();
    assert!(error.contains("fixture failure before task and run"),"{error}");
    let linked: Option<String> = db.query_row(
        "SELECT overseer_run_id FROM swarm_director_owners WHERE run_id=?1",
        [id], |row| row.get(0)).unwrap();
    assert!(linked.is_none());
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"})).is_err(),
        "an unlinked supervised launch must remain reserved until reconciled");
    d.kill9();
    d.spawn();
    let recovered = d.call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_no_spawn"}));
    assert_eq!(recovered["generation"],2);
    assert_eq!(recovered["replacement_pending"],true);
}

#[test]
fn uncertain_spawn_remains_reserved_without_a_process_record() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("uncertain-spawn-source"));
    let run = d.call("swarm.create", json!({"category":"Uncertain director spawn",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_process_record BEFORE UPDATE OF run_dir ON runs
        WHEN NEW.title='Uncertain director' BEGIN
        SELECT RAISE(ABORT, 'fixture failure after spawn requested'); END;").unwrap();
    let launched = d.call("swarm.director.launch", json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep",
        "args":["1"],"prompt":"Audit backend","title":"Uncertain director"}));
    assert_eq!(launched["status"],"launch_uncertain");
    assert!(launched["error"].as_str().unwrap().contains("fixture failure after spawn requested"));
    let process = launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.run(process)["status"],"queued");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stall_reason"],
        "director_termination_unknown");
    let (phase, linked): (String,Option<String>) = db.query_row(
        "SELECT launch_phase,overseer_run_id FROM swarm_director_owners WHERE run_id=?1",
        [id], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
    assert_eq!(phase,"spawn_requested");
    assert!(linked.is_some());
    d.kill9();
    d.spawn();
    assert_eq!(d.run(process)["status"],"queued");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stall_reason"],
        "director_termination_unknown");
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_no_spawn"})).is_err(),
        "a child may exist even when the run directory update was lost");
}

#[test]
fn orphaned_supervisor_is_reattached_by_verified_director_identity() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("reattach-director-source"));
    let run = d.call("swarm.create", json!({"category":"Reattach director",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_orphan_record BEFORE UPDATE OF run_dir ON runs
        WHEN NEW.title='Reattach director' BEGIN
        SELECT RAISE(ABORT, 'fixture orphaned director supervisor'); END;").unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":id,"generation":1,
        "repo":checkout,"program":"/bin/sleep","args":["30"],
        "prompt":"Audit backend","title":"Reattach director"}));
    assert_eq!(launched["status"],"launch_uncertain");
    let process = launched["overseer_run_id"].as_str().unwrap();
    let run_dir = d.home.path().join("runs").join(process).join("p1");
    let deadline = Instant::now()+Duration::from_secs(3);
    while !run_dir.join("shim.json").exists() {
        assert!(Instant::now()<deadline,"orphan supervisor did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    db.execute_batch("DROP TRIGGER abort_orphan_record;").unwrap();
    d.kill9();
    d.spawn();
    let linked_dir: String = db.query_row("SELECT run_dir FROM runs WHERE id=?1",
        [process],|row|row.get(0)).unwrap();
    assert_eq!(linked_dir,run_dir.to_string_lossy());
    assert_eq!(d.run(process)["status"],"running");
    assert_ne!(d.call("swarm.get",json!({"id":id}))["stall_reason"],
        "director_termination_unknown");
    d.call("run.interrupt",json!({"run_id":process}));
    let stopped=d.wait_done(process,8);
    assert_ne!(stopped["status"],"disconnected","{stopped}");
    assert!(run_dir.join("exit.json").exists());
    let recovered=d.call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"}));
    assert_eq!(recovered["generation"],2);
}

#[test]
fn orphaned_supervisor_with_mismatched_credential_stays_uncertain() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("untrusted-orphan-source"));
    let run = d.call("swarm.create", json!({"category":"Untrusted orphan",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_untrusted_orphan BEFORE UPDATE OF run_dir ON runs
        WHEN NEW.title='Untrusted director' BEGIN
        SELECT RAISE(ABORT, 'fixture untrusted orphan'); END;").unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":id,"generation":1,
        "repo":checkout,"program":"/bin/sleep","args":["30"],
        "prompt":"Audit backend","title":"Untrusted director"}));
    assert_eq!(launched["status"],"launch_uncertain");
    let process = launched["overseer_run_id"].as_str().unwrap();
    let dir = d.home.path().join("runs").join(process).join("p1");
    let deadline = Instant::now()+Duration::from_secs(3);
    while !dir.join("shim.json").exists() {
        assert!(Instant::now()<deadline,"orphan supervisor did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let launch_path=dir.join("launch.json");
    let original=std::fs::read(&launch_path).unwrap();
    let mut altered: serde_json::Value=serde_json::from_slice(&original).unwrap();
    altered["env"]["OVERSEER_SWARM_DIRECTOR_TOKEN"]=json!("wrong-owner-token");
    std::fs::write(&launch_path,serde_json::to_vec(&altered).unwrap()).unwrap();
    db.execute_batch("DROP TRIGGER abort_untrusted_orphan;").unwrap();
    d.kill9();
    d.spawn();
    let linked: Option<String>=db.query_row("SELECT run_dir FROM runs WHERE id=?1",
        [process],|row|row.get(0)).unwrap();
    assert!(linked.is_none());
    assert_eq!(d.run(process)["status"],"queued");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stall_reason"],
        "director_termination_unknown");
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_no_spawn"})).is_err());
    std::fs::write(&launch_path,original).unwrap();
    d.kill9();
    d.spawn();
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
}

#[test]
fn exited_orphan_supervisor_replays_its_exit_once_after_restart() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("exited-orphan-source"));
    let run = d.call("swarm.create", json!({"category":"Exited orphan",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_exited_orphan BEFORE UPDATE OF run_dir ON runs
        WHEN NEW.title='Exited director' BEGIN
        SELECT RAISE(ABORT, 'fixture exited orphan'); END;").unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":id,"generation":1,
        "repo":checkout,"program":"/usr/bin/true","args":[],
        "prompt":"Audit backend","title":"Exited director"}));
    assert_eq!(launched["status"],"launch_uncertain");
    let process=launched["overseer_run_id"].as_str().unwrap();
    let dir=d.home.path().join("runs").join(process).join("p1");
    let deadline=Instant::now()+Duration::from_secs(3);
    while !dir.join("exit.json").exists() {
        assert!(Instant::now()<deadline,"orphan supervisor did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    db.execute_batch("DROP TRIGGER abort_exited_orphan;").unwrap();
    d.kill9();
    d.spawn();
    let finished=d.wait_done(process,8);
    assert_eq!(finished["status"],"completed","{finished}");
    let recovered=d.call("swarm.director.recover",json!({"run_id":id,
        "generation":1,"revision":0,"termination":"confirmed_dead"}));
    assert_eq!(recovered["generation"],2);
    let events=d.events(process);
    assert_eq!(events.iter().filter(|e|e["kind"]=="status"
        && e["payload"]["status"]=="completed").count(),1);
    d.kill9();
    d.spawn();
    assert_eq!(d.events(process).iter().filter(|e|e["kind"]=="status"
        && e["payload"]["status"]=="completed").count(),1);
}
