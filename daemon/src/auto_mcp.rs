//! Per-run stdio MCP surface for an Auto-enabled parent. The model sees only
//! bounded submit/result tools; the daemon derives the parent and account set
//! from the issued run capability.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

const MAX_FRAME: u64 = 1024 * 1024;

fn daemon_call(socket: &Path, method: &str, params: Value) -> Result<Value> {
    let mut conn = UnixStream::connect(socket)?;
    conn.set_read_timeout(Some(Duration::from_secs(20)))?;
    conn.set_write_timeout(Some(Duration::from_secs(20)))?;
    conn.write_all(format!("{}\n", json!({"id":1,"method":method,"params":params})).as_bytes())?;
    let mut reader = BufReader::new(conn);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.len() as u64 > MAX_FRAME { bail!("Auto response exceeded its bound"); }
    let response: Value = serde_json::from_str(&line)?;
    if let Some(error) = response.get("error") {
        bail!("{}", error["message"].as_str().unwrap_or("Auto bridge call failed"));
    }
    Ok(response["result"].clone())
}

fn tools() -> Value {
    json!({"tools":[
        {"name":"auto_submit","description":"Delegate one bounded work unit to an eligible Overseer-managed agent. Use a stable work_unit_id when retrying a lost response. The daemon chooses the route and enforces the parent's limits.",
         "inputSchema":{"type":"object","additionalProperties":false,
           "required":["work_unit_id","prompt","min_tier","required_tools"],
           "properties":{
             "work_unit_id":{"type":"string","minLength":1,"maxLength":120},
             "prompt":{"type":"string","minLength":1,"maxLength":32768},
             "title":{"type":"string","maxLength":80},
             "min_tier":{"type":"string","enum":["general","frontier"]},
             "required_tools":{"type":"array","maxItems":16,"items":{"type":"string"}},
             "context_needed":{"type":"integer","minimum":0},
             "requires_approvals":{"type":"boolean"},
             "sandbox":{"type":"string","enum":["read_only","workspace_write"]},
             "execution_budget_ms":{"type":"integer","minimum":1000,"maximum":300000},
             "task_class":{"type":"string","enum":["browser_check","routine_edit","difficult_diagnosis","general"]}
           }}},
        {"name":"auto_result","description":"Read the settled result of one child returned by auto_submit. A pending result may be read again; this does not repeat the child.",
         "inputSchema":{"type":"object","additionalProperties":false,
           "required":["child_run_id"],
           "properties":{"child_run_id":{"type":"string","minLength":1,"maxLength":120}}}}
    ]})
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub fn run(run_id: &str, capability_path: &Path, socket: &Path) -> Result<()> {
    let capability = std::fs::read_to_string(capability_path)?;
    let capability = capability.trim();
    if capability.len() != 64 || !capability.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid Auto run capability");
    }
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    loop {
        let mut line = Vec::new();
        let n = reader.by_ref().take(MAX_FRAME + 1).read_until(b'\n', &mut line)?;
        if n == 0 { break; }
        if line.len() as u64 > MAX_FRAME {
            writeln!(writer, "{}", error(Value::Null, -32700, "MCP request exceeded its bound"))?;
            writer.flush()?;
            break;
        }
        let message: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                writeln!(writer, "{}", error(Value::Null, -32700, "invalid MCP JSON"))?;
                writer.flush()?;
                continue;
            }
        };
        let Some(id) = message.get("id").cloned() else { continue; };
        let method = message["method"].as_str().unwrap_or_default();
        let answer = match method {
            "initialize" => {
                let version = message["params"]["protocolVersion"].as_str().unwrap_or("2025-03-26");
                let version = if matches!(version, "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25") {
                    version
                } else { "2025-03-26" };
                response(id, json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},
                    "serverInfo":{"name":"overseer-auto","version":env!("CARGO_PKG_VERSION")}}))
            }
            "ping" => response(id, json!({})),
            "tools/list" => response(id, tools()),
            "resources/list" => response(id, json!({"resources":[]})),
            "prompts/list" => response(id, json!({"prompts":[]})),
            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                let args = message["params"]["arguments"].as_object();
                let result = (|| -> Result<Value> {
                    let mut params = Value::Object(args.cloned().ok_or_else(|| anyhow!("tool arguments must be an object"))?);
                    params["run_id"] = json!(run_id);
                    params["capability"] = json!(capability);
                    match name {
                        "auto_submit" => daemon_call(socket, "auto.bridge.submit", params),
                        "auto_result" => daemon_call(socket, "auto.bridge.result", params),
                        _ => bail!("unknown Auto tool"),
                    }
                })();
                match result {
                    Ok(value) => response(id, json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})),
                    Err(failure) => {
                        let message: String = crate::redact::redact(&failure.to_string()).chars().take(500).collect();
                        response(id, json!({"content":[{"type":"text","text":message}],"isError":true}))
                    }
                }
            }
            _ => error(id, -32601, "unsupported MCP method"),
        };
        writeln!(writer, "{answer}")?;
        writer.flush()?;
    }
    Ok(())
}
