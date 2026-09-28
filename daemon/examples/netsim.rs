//! The simulated network of the Continuity tests, as a program for the live checks (a test tool;
//! it is not shipped). It listens on loopback and follows a control file:
//!
//!   cargo run -p overseerd --example netsim -- <control-file> [127.0.0.1:port]
//!
//! The control file holds one word, re-read ten times a second: `online` (probes answered,
//! `CONNECT` tunnelled to the real host), `refuse` (nothing listens; open connections end),
//! `blackhole` (accepted, never answered) or `drop` (accepted and closed). The first line printed
//! is `listening 127.0.0.1:<port>`; then one line per connection or change.
#[path = "../tests/common/netsim.rs"]
mod netsim;

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let control = args.first().expect("usage: netsim <control-file> [127.0.0.1:port]").clone();
    let sim = netsim::NetSim::start_at(args.get(1).map(|s| s.as_str()).unwrap_or("127.0.0.1:0"));
    println!("listening {}", sim.addr);
    let _ = std::io::stdout().flush();
    let mut printed = 0;
    loop {
        if let Ok(word) = std::fs::read_to_string(&control) {
            if let Some(mode) = netsim::Mode::parse(&word) {
                if mode != sim.mode() {
                    sim.set(mode);
                }
            }
        }
        let log = sim.log();
        for line in &log[printed..] {
            println!("{line}");
        }
        printed = log.len();
        let _ = std::io::stdout().flush();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
