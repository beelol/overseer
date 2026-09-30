//! AC-243: after a merge, the agent's tile reads "Merged into main (commit)" and offers Clean up.
mod support;

use serde_json::json;
use support::*;

#[test]
fn ac243_a_merged_agent_reads_merged_into_main_with_its_commit() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let web = repo(&t.path().join("web-app"));
    let run = d.sh(&web, "Add the features list", "mkdir -p data && printf 'export const features = [];\\n' > data/features.js");
    d.wait_status(&run, |s| s == "completed", 20);
    let ws = d.run(&run)["workspace_id"].as_str().unwrap().to_string();
    assert_eq!(d.ctl("workspace.merge_prepare", json!({ "workspace_id": ws }))["state"], "ready");
    let done = d.ctl("workspace.merge_complete", json!({ "workspace_id": ws }));
    let commit: String = done["commit"].as_str().unwrap().chars().take(7).collect();
    let mut tui = Tui::attach(&d, 160, 40);
    let s = tui.until_screen(15, &format!("Merged into main ({commit})"));
    assert!(s.contains("C clean up"), "offers Clean up:\n{s}");
    tui.snapshot("ac243-merged");
}
