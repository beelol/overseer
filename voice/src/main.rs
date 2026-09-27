//! overseer-listener: Overseer's voice listener, started by the daemon (Voice Mode, Gate R).
//!
//!     overseer-listener [--input mic|mic-plain|file:<wav>|stdin|feed] [--fast]
//!                       [--model <ggml.bin> | --script <words.json> | --no-words]
//!                       [--hint <words>] [--voice <name>] [--rate <wpm>]
//!                       [--commands <timed.jsonl>] [--echo <gain>] [--no-control]
//!
//! Events go out as JSON lines on standard output; commands come in as JSON lines on standard
//! input (unless the input itself is standard input). See src/protocol.rs.

use anyhow::{bail, Context, Result};
use overseer_listener::listener::{self, Input, Options, Pace};
use overseer_listener::protocol::Command;
use overseer_listener::recognize::{Recognizer, ScriptLine, Scripted, Whisper};
use std::path::PathBuf;

fn main() {
    if let Err(e) = run() {
        let msg = serde_json::json!({"type": "error", "message": format!("{e:#}"), "t_ms": 0});
        println!("{msg}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut opts = Options::default();
    let mut recognizer: Option<Box<dyn Recognizer>> = None;
    let mut words = true;
    let mut model: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        let mut value = || args.next().with_context(|| format!("{a} needs a value"));
        match a.as_str() {
            "--input" => {
                let v = value()?;
                opts.input = match v.as_str() {
                    "mic" => Input::Mic {
                        voice_processing: true,
                    },
                    "mic-plain" => Input::Mic {
                        voice_processing: false,
                    },
                    "stdin" => Input::Stdin,
                    "feed" => Input::Feed,
                    other => match other.strip_prefix("file:") {
                        Some(p) => Input::File(PathBuf::from(p)),
                        None => bail!("unknown input {other}"),
                    },
                };
            }
            "--fast" => opts.pace = Pace::Fast,
            "--model" => model = Some(PathBuf::from(value()?)),
            "--script" => {
                let lines: Vec<ScriptLine> = serde_json::from_slice(&std::fs::read(value()?)?)?;
                recognizer = Some(Box::new(Scripted { lines }));
            }
            "--no-words" => words = false,
            "--hint" => opts.hint = value()?,
            "--voice" => opts.voice = Some(value()?),
            "--rate" => opts.rate = Some(value()?.parse()?),
            "--echo" => opts.echo = value()?.parse()?,
            "--no-control" => opts.control = false,
            "--commands" => {
                let text = std::fs::read_to_string(value()?)?;
                for line in text.lines().filter(|l| !l.trim().is_empty()) {
                    let mut v: serde_json::Value = serde_json::from_str(line)?;
                    let at = v["at_ms"].as_u64().context("a timed command needs at_ms")?;
                    v.as_object_mut().unwrap().remove("at_ms");
                    opts.timed.push((at, serde_json::from_value::<Command>(v)?));
                }
            }
            other => bail!("unknown argument {other}"),
        }
    }
    if recognizer.is_none() && words {
        let path =
            model.context("a model is needed: --model <ggml.bin> (or --script, or --no-words)")?;
        recognizer = Some(Box::new(
            Whisper::load(&path).with_context(|| format!("loading {}", path.display()))?,
        ));
    }
    listener::run(opts, recognizer, std::io::stdout())
}
