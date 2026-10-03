//! Internal external-program boundary; unavailable until isolation is qualified.
//! No public RPC or native hook calls this module at this checkpoint.
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub struct Program {
    pub path: PathBuf,
    pub sha256: String,
}

pub struct Request {
    pub program: Program,
    pub scratch: PathBuf,
    pub args: Vec<String>,
    pub deadline: Duration,
    pub max_output_bytes: usize,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Output(Vec<u8>),
    Bypass(&'static str),
}

pub fn run(_request: &Request, _input: &[u8], _cancel: &AtomicBool) -> Outcome {
    Outcome::Bypass("runner_unavailable")
}
