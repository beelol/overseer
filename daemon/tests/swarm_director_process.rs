mod common;

use common::*;
use serde_json::json;
use std::time::{Duration, Instant};

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
