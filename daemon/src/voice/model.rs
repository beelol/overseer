//! The speech model (AC-162, AC-173): whisper.cpp's English models, downloaded only when the owner
//! asks (`voice.download`, after the size is shown), checked against a pinned SHA-256, and kept
//! under the daemon's data folder. Loaded only by the listener, inside the memory budget.

use super::Voice;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use sha2::Digest;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// The models Voice Mode can use: name, size in bytes, SHA-256.
const MODELS: &[(&str, u64, &str)] = &[
    (
        "small.en",
        487_614_201,
        "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
    ),
    (
        "base.en",
        147_964_211,
        "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
    ),
];
pub const NAMES: &[&str] = &["small.en", "base.en"];

pub fn path(name: &str) -> PathBuf {
    if let Some(p) = std::env::var_os("OVERSEER_VOICE_MODEL") {
        return PathBuf::from(p);
    }
    crate::paths::data_dir()
        .join("voice")
        .join("models")
        .join(format!("ggml-{name}.bin"))
}

pub fn size(name: &str) -> u64 {
    MODELS
        .iter()
        .find(|m| m.0 == name)
        .map(|m| m.1)
        .unwrap_or(0)
}

/// Memory a model takes once loaded: whisper.cpp's own figures (base 388 MB, small 852 MB).
pub fn memory_needed(name: &str) -> u64 {
    match name {
        "base.en" => 388_000_000,
        "small.en" => 852_000_000,
        other => size(other).max(500_000_000) * 2,
    }
}

/// A speech model is loaded only inside Gate L's memory budget (AC-140, AC-173), checked before
/// the listener starts. `OVERSEER_TEST_MEMORY` sets the machine's memory in tests.
pub fn check_budget(name: &str) -> Result<()> {
    let mem = crate::sys::memory()?;
    let budget = crate::local::budget(&mem, &crate::continuity::settings().pick_options(false), 0);
    let need = memory_needed(name);
    if need > budget.budget {
        bail!(
            "the speech model {name} needs {} MB and the memory budget is {} MB now; Voice Mode waits for memory",
            need / 1_000_000,
            budget.budget / 1_000_000
        );
    }
    Ok(())
}

/// Starts the download in the background; progress goes on the live channel.
pub fn download(v: &Arc<Voice>, name: &str) -> Result<Value> {
    let (_, bytes, sha) = *MODELS
        .iter()
        .find(|m| m.0 == name)
        .ok_or_else(|| anyhow!("unknown model {name}"))?;
    let target = path(name);
    if target.exists() {
        return Ok(json!({"downloaded": true, "bytes": bytes}));
    }
    if v.st
        .lock()
        .unwrap()
        .download
        .as_ref()
        .is_some_and(|d| d["state"] == "downloading")
    {
        return Ok(json!({"downloading": true}));
    }
    let url = std::env::var("OVERSEER_VOICE_MODEL_URL").unwrap_or_else(|_| {
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin")
    });
    v.st.lock().unwrap().download =
        Some(json!({"state": "downloading", "received": 0, "bytes": bytes}));
    let v = v.clone();
    let name = name.to_string();
    std::thread::spawn(move || {
        let result = fetch(&v, &url, &target, bytes, sha);
        let state = match &result {
            Ok(()) => json!({"state": "done", "bytes": bytes}),
            Err(e) => json!({"state": "failed", "reason": e.to_string()}),
        };
        v.st.lock().unwrap().download = Some(state.clone());
        v.emit(json!({"kind": "download", "model": name, "progress": state}));
    });
    Ok(json!({"downloading": true, "bytes": bytes}))
}

fn fetch(v: &Arc<Voice>, url: &str, target: &PathBuf, bytes: u64, sha: &str) -> Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| anyhow!("no folder for the model"))?;
    crate::paths::ensure_private_dir(dir)?;
    let part = target.with_extension("part");
    let resp = ureq::get(url).call()?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part)?;
    let mut hash = sha2::Sha256::new();
    let mut received = 0u64;
    let mut buf = vec![0u8; 1 << 16];
    let mut last = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hash.update(&buf[..n]);
        received += n as u64;
        if received > bytes {
            let _ = std::fs::remove_file(&part);
            bail!("the download is larger than the model");
        }
        if last.elapsed().as_millis() >= 500 {
            last = std::time::Instant::now();
            let p = json!({"state": "downloading", "received": received, "bytes": bytes});
            v.st.lock().unwrap().download = Some(p.clone());
            v.emit(json!({"kind": "download", "progress": p}));
        }
    }
    file.sync_all()?;
    let got = hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if received != bytes || got != sha {
        let _ = std::fs::remove_file(&part);
        bail!("the model did not match its checksum and was deleted");
    }
    std::fs::rename(&part, target)?;
    Ok(())
}
