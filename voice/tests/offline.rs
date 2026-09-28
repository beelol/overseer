//! AC-173: with the network off and nothing writable, the listener still speaks and recognizes
//! speech: everything happens on the Mac, in memory. A macOS sandbox kills the listener at its
//! first connection or file write, so finishing at all is the proof.
//!
//! Recognition needs a speech model: `OVERSEER_LISTENER_TEST_MODEL=<ggml-base.en.bin>` runs it.
//! Loading the model on the GPU writes macOS's shader cache (the user cache folder,
//! `com.apple.metal`) and opens the listener's own folder for writing (the GPU backend looking for
//! its shader library; a folder takes no data). Those two are allowed, and the cache is checked
//! afterwards for audio.

use overseer_listener::pcm;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn profile(also: Option<&Path>) -> String {
    let exe_dir = Path::new(env!("CARGO_BIN_EXE_overseer-listener"))
        .parent()
        .unwrap();
    let extra = also
        .map(|p| {
            format!(
                " (subpath \"{}\") (literal \"{}\")",
                p.display(),
                std::fs::canonicalize(exe_dir).unwrap().display()
            )
        })
        .unwrap_or_default();
    format!(
        "(version 1)(allow default)\
         (deny network* (with send-signal SIGKILL))\
         (deny file-write* (with send-signal SIGKILL))\
         (allow file-write* (subpath \"/dev\"){extra})"
    )
}

fn boxed(args: &[String], also: Option<&Path>) -> Output {
    Command::new("/usr/bin/sandbox-exec")
        .arg("-p")
        .arg(profile(also))
        .arg(env!("CARGO_BIN_EXE_overseer-listener"))
        .args(args)
        .output()
        .unwrap()
}

fn not_killed(out: &Output) {
    assert_eq!(
        out.status.signal(),
        None,
        "killed: it wrote a file or connected ({})",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The user cache folder, as the sandbox sees it (no symbolic links).
fn cache_dir() -> PathBuf {
    let out = Command::new("getconf")
        .arg("DARWIN_USER_CACHE_DIR")
        .output()
        .unwrap();
    std::fs::canonicalize(String::from_utf8_lossy(&out.stdout).trim()).unwrap()
}

/// Files under `dir` changed since `since` that hold audio.
fn audio_since(dir: &Path, since: std::time::SystemTime) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(p);
            } else if meta.modified().is_ok_and(|m| m >= since) {
                let head = std::fs::read(&p)
                    .map(|b| b.into_iter().take(12).collect::<Vec<u8>>())
                    .unwrap_or_default();
                let audio = head.starts_with(b"RIFF")
                    || head.starts_with(b"FORM")
                    || head.starts_with(b"caff")
                    || ["wav", "aif", "aiff", "caf", "m4a", "mp3"]
                        .iter()
                        .any(|x| p.extension().is_some_and(|e| e == *x));
                if audio {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn events(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[test]
fn offline_and_writing_nothing_overseer_still_speaks() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("room.wav");
    std::fs::write(&wav, pcm::wav_bytes(&vec![0.0; 16_000 * 10])).unwrap();
    let commands = dir.path().join("commands.jsonl");
    std::fs::write(
        &commands,
        r#"{"at_ms": 200, "cmd": "speak", "line": 1, "text": "On it. Telling Phone to wait for the review."}"#,
    )
    .unwrap();
    let out = boxed(
        &[
            "--input".into(),
            format!("file:{}", wav.display()),
            "--no-words".into(),
            "--no-control".into(),
            "--commands".into(),
            commands.display().to_string(),
        ],
        None,
    );
    not_killed(&out);
    let e = events(&out);
    let spoke: Vec<&str> = e
        .iter()
        .filter(|v| v["type"] == "spoke")
        .filter_map(|v| v["event"].as_str())
        .collect();
    assert!(spoke.contains(&"start"), "{e:?}");
    assert!(!e.iter().any(|v| v["type"] == "error"), "{e:?}");
}

#[test]
fn offline_and_writing_nothing_the_model_still_recognizes() {
    let Some(model) = std::env::var_os("OVERSEER_LISTENER_TEST_MODEL") else {
        eprintln!("skipped: OVERSEER_LISTENER_TEST_MODEL names no speech model");
        return;
    };
    // Speech made at test time by this test (not the listener), in a temporary folder.
    let Ok(speech) =
        overseer_listener::speak::say("Tell the phone app to wait for the review.", None, None)
    else {
        eprintln!("skipped: no speech synthesizer");
        return;
    };
    let mut audio = vec![0.0; 16_000];
    audio.extend(speech.iter().map(|s| s * 0.6));
    audio.extend(vec![0.0; 16_000 * 2]);
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("speech.wav");
    std::fs::write(&wav, pcm::wav_bytes(&audio)).unwrap();
    let cache = cache_dir();
    let since = std::time::SystemTime::now();
    let out = boxed(
        &[
            "--input".into(),
            format!("file:{}", wav.display()),
            "--fast".into(),
            "--no-control".into(),
            "--model".into(),
            model.to_string_lossy().to_string(),
        ],
        Some(&cache),
    );
    not_killed(&out);
    let written = audio_since(&cache, since);
    assert!(written.is_empty(), "audio in the cache folder: {written:?}");
    let e = events(&out);
    let words: String = e
        .iter()
        .filter(|v| v["type"] == "utterance")
        .filter_map(|v| v["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert!(
        words.contains("phone") && words.contains("review"),
        "{words:?}: {e:?}"
    );
}
