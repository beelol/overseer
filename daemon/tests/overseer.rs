//! Overseer itself (Gate S): the daemon half and the tools it gives a run.
mod common;
use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Runs `overseerd mcp` as a harness would (an MCP server over stdio) and replays a recorded
/// JSON-RPC exchange, returning one reply per request.
fn mcp_exchange(d: &Daemon, token: &str, requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(BIN)
        .args(["mcp", "--socket", &d.socket().display().to_string()])
        .env("OVERSEER_MCP_TOKEN", token)
        .env_clear()
        .env("OVERSEER_MCP_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        stdin.write_all(format!("{r}\n").as_bytes()).unwrap();
    }
    drop(stdin);
    let out = BufReader::new(child.stdout.take().unwrap());
    let replies: Vec<Value> = out.lines().map(|l| serde_json::from_str(&l.unwrap()).unwrap()).collect();
    child.wait().unwrap();
    replies
}

/// AC-180: the recorded exchange of the spike (initialize, initialized, tools/list, tools/call
/// roster), replayed through the real shim against a real daemon. The shim serves what the
/// daemon holds and nothing of its own.
#[test]
fn ac180_mcp_shim_serves_overseers_tools_from_the_daemon() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo hi"]);
    let run = run_id(&created);
    d.wait_done(&run, 20);
    let token = d.call("overseer.token", json!({"run_id": "t-overseer", "role": "overseer"}))["token"].as_str().unwrap().to_string();
    let replies = mcp_exchange(
        &d,
        &token,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "roster", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "agent", "arguments": {"id": run}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "shell", "arguments": {"command": "ls"}}}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "nonsense"}),
        ],
    );
    // A notification gets no reply: six replies for seven messages.
    assert_eq!(replies.len(), 6, "{replies:?}");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "overseer");
    assert_eq!(replies[0]["result"]["capabilities"]["tools"]["listChanged"], false);
    let tools: Vec<&str> = replies[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(tools, ["roster", "agent"]);
    let roster = replies[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(roster.contains(&run) && roster.contains("completed"), "roster names the run: {roster}");
    assert_eq!(replies[2]["result"]["isError"], false);
    let digest = replies[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(digest.contains(&format!("id: {run}")) && digest.contains("status: completed"), "{digest}");
    // A tool the daemon does not give this role is refused as a tool error the model can read.
    assert_eq!(replies[4]["result"]["isError"], true);
    assert!(replies[4]["result"]["content"][0]["text"].as_str().unwrap().contains("no tool shell"));
    assert_eq!(replies[5]["error"]["code"], -32601);
    // Every call is an event on the calling run, with the role and the tool.
    let events = d.events("t-overseer");
    let calls: Vec<&str> = events.iter().filter(|e| e["kind"] == "overseer_tool_call").map(|e| e["payload"]["name"].as_str().unwrap()).collect();
    assert_eq!(calls, ["roster", "agent"]);
}

/// Who is speaking comes from the token, never from the text: an unknown token gets nothing,
/// and an agent's token does not open Overseer's tools.
#[test]
fn ac180_tokens_decide_who_may_call_what() {
    let d = Daemon::start(&[]);
    let err = d.try_call("overseer.tools", json!({"token": "made-up"})).unwrap_err();
    assert!(err.contains("unknown token"), "{err}");
    let replies = mcp_exchange(&d, "made-up", &[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})]);
    assert!(replies[0]["error"]["message"].as_str().unwrap().contains("unknown token"));
    let agent = d.call("overseer.token", json!({"run_id": "r-agent", "role": "agent"}))["token"].as_str().unwrap().to_string();
    let tools = d.call("overseer.tools", json!({"token": agent}));
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["roster"]);
    let err = d.try_call("overseer.tool", json!({"token": agent, "name": "agent", "arguments": {"id": "x"}})).unwrap_err();
    assert!(err.contains("no tool agent"), "{err}");
    assert!(d.try_call("overseer.token", json!({"run_id": "r", "role": "king"})).is_err());
}
