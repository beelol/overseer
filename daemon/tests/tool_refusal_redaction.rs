//! AC-200: refusals are model-visible traffic and must redact credentials too.
mod common;
use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TOKEN: &str = "sk-proj-syntheticSYNTHETICabcdefghijkl012345";

struct McpChild(Child);

impl Drop for McpChild {
    fn drop(&mut self) {
        // Also runs if an assertion or pipe operation panics.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn mcp_call(socket: &Path, token: &str, name: &str, arguments: Value) -> Value {
    let mut child = McpChild(
        Command::new(BIN)
            .arg("mcp")
            .arg("--socket")
            .arg(socket)
            .env_clear()
            .env("OVERSEER_MCP_TOKEN", token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":name,"arguments":arguments}})).unwrap();
    drop(input);
    let output = child.0.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(output).read_line(&mut line).map(|_| line);
        let _ = send.send(result);
    });
    let output = match receive.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => {
            reader.join().unwrap();
            result.unwrap()
        }
        Err(error) => {
            let _ = child.0.kill();
            let _ = child.0.wait();
            reader.join().unwrap();
            panic!("MCP reply did not arrive: {error}");
        }
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "MCP shim exited with {status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "MCP shim did not exit after stdin closed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    serde_json::from_str(&output).unwrap()
}

fn assert_redacted(text: &str) {
    assert!(
        !text.contains(TOKEN),
        "synthetic credential leaked in refusal: {text}"
    );
    assert!(
        text.contains("[redacted]"),
        "redaction must preserve the refusal context: {text}"
    );
    assert!(
        text.contains("refused") || text.contains("not a path"),
        "{text}"
    );
}

#[test]
fn ac200_daemon_and_mcp_refusals_redact_invalid_claim_action_and_file() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let run = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo safe"]));
    d.wait_done(&run, 20);
    let agent_token = d.call("overseer.token", json!({"run_id":run,"role":"agent"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    let overseer_token = d.call("overseer.token", json!({"run_id":run,"role":"overseer"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    for (token, name, arguments, result_error) in [
        (
            &agent_token,
            "claim",
            json!({"paths":[format!("../{TOKEN}")]}),
            true,
        ),
        (
            &overseer_token,
            "propose",
            json!({"actions":[{"action":TOKEN}]}),
            true,
        ),
        (
            &overseer_token,
            "file",
            json!({"id":run,"path":format!("../{TOKEN}")}),
            false,
        ),
    ] {
        let direct = d.try_call(
            "overseer.tool",
            json!({"token":token,"name":name,"arguments":arguments}),
        );
        if result_error {
            let response = direct.unwrap();
            assert_eq!(response["is_error"], true, "{response}");
            assert_redacted(response["text"].as_str().unwrap());
        } else {
            assert_redacted(&direct.unwrap_err());
        }
        let reply = mcp_call(&d.socket(), token, name, arguments);
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert_redacted(reply["result"]["content"][0]["text"].as_str().unwrap());
    }
    let forged = d.try_call(
        "overseer.tool",
        json!({"token":"forged", "name":"roster", "arguments":{}}),
    );
    assert!(forged.unwrap_err().contains("unknown token"));
}

#[test]
fn ac200_mcp_fallback_redacts_daemon_transport_errors() {
    // A synthetic socket peer proves the shim's independent error boundary, even when
    // the daemon response itself was not sanitized (e.g. a legacy peer).
    let r = tmp();
    let socket = r.path().join("peer.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let reply = std::thread::scope(|scope| {
        let peer = scope.spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "MCP shim never connected");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("synthetic socket accept failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "overseer.tool");
            writeln!(
                stream,
                "{}",
                json!({"id":1,"error":{"message":format!("bad synthetic path {TOKEN}")}})
            )
            .unwrap();
        });
        let reply = mcp_call(&socket, "synthetic-tool-token", "file", json!({}));
        peer.join().unwrap();
        reply
    });
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert_redacted(reply["result"]["content"][0]["text"].as_str().unwrap());
}
