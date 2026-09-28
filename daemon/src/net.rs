//! Platform boundary for connectivity (Continuity, AC-83). Two of the three sources the
//! connection state is decided from live here:
//!
//! 1. **The system's own answer**: is this machine connected? macOS asks the SystemConfiguration
//!    framework for the reachability of the default route; Linux asks NetworkManager, or falls
//!    back to the default route and the carrier state.
//! 2. **Probes** that send no credentials and no payload: a certificate-verified baseline by name
//!    and by IP (so a DNS failure, a routing failure and a captive portal are told apart) and one
//!    probe per provider to the hosts its harness uses. Any HTTP answer, 401 and 403 included,
//!    means reachable.
//!
//! The third source, errors from the agents themselves, is gathered in `continuity.rs`.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SystemNet {
    Connected,
    NoNetwork,
    Unknown,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SystemAnswer {
    pub state: SystemNet,
    /// What the system said, in its own terms (flags, connectivity word, route).
    pub detail: String,
}

/// One probe's outcome. `reason` is `answered <status>` when reachable, otherwise the failure
/// class: `dns`, `connect`, `tls`, `timeout`, `outage` or `other`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub ok: bool,
    pub reason: String,
}

impl Probe {
    fn fixture(v: &Value) -> Self {
        match v {
            Value::Bool(true) => Self { ok: true, reason: "answered (fixture)".into() },
            Value::String(reason) => Self { ok: false, reason: reason.clone() },
            _ => Self { ok: false, reason: "connect".into() },
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Baseline {
    pub by_name: Probe,
    pub by_ip: Probe,
}

impl Baseline {
    /// Providers are reached by name, so the baseline holds only when the name probe does.
    pub fn ok(&self) -> bool {
        self.by_name.ok
    }

    /// Why there is no working connection, in the words the UI shows.
    pub fn failure(&self) -> &'static str {
        if self.by_name.ok {
            "working"
        } else if self.by_name.reason == "tls" || self.by_ip.reason == "tls" {
            "captive portal"
        } else if self.by_ip.ok {
            "DNS is not answering"
        } else {
            "no route to the internet"
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Reading {
    pub system: SystemAnswer,
    /// `None` when probes are off.
    pub baseline: Option<Baseline>,
    /// Per provider (`openai`, `anthropic`); empty when probes are off.
    pub providers: BTreeMap<String, Probe>,
}

/// The hosts each provider's harness talks to. A provider is reachable when all of them answer.
pub const PROVIDERS: &[(&str, &[&str])] = &[("openai", &["https://chatgpt.com/", "https://api.openai.com/"]), ("anthropic", &["https://api.anthropic.com/", "https://claude.ai/"])];
const BASELINE_NAME: &str = "https://www.google.com/generate_204";
const BASELINE_IP: &str = "https://1.1.1.1/";

/// One round: the system's answer, and the probes when they are on and the system is not
/// already certain there is no network. `OVERSEER_TEST_NET` names a JSON file read instead, e.g.
/// `{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers":
/// {"openai": "connect", "anthropic": true}}` (a string is the failure class). The file is read
/// on every call, so a test changes the network by rewriting it.
pub fn read(probes: bool) -> Reading {
    let system = system();
    if !probes || system.state == SystemNet::NoNetwork {
        return Reading { system, baseline: None, providers: BTreeMap::new() };
    }
    let (baseline, providers) = probe_round();
    Reading { system, baseline, providers }
}

fn fixture_file() -> Option<Value> {
    let path = std::env::var_os("OVERSEER_TEST_NET")?;
    Some(std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null))
}

/// The baseline and every provider, probed in parallel.
pub fn probe_round() -> (Option<Baseline>, BTreeMap<String, Probe>) {
    if let Some(v) = fixture_file() {
        let baseline = v["baseline"].is_object().then(|| Baseline { by_name: Probe::fixture(&v["baseline"]["by_name"]), by_ip: Probe::fixture(&v["baseline"]["by_ip"]) });
        let providers = v["providers"].as_object().map(|m| m.iter().map(|(k, p)| (k.clone(), Probe::fixture(p))).collect()).unwrap_or_default();
        return (baseline, providers);
    }
    let mut urls: Vec<(String, &str)> = vec![("baseline:name".into(), BASELINE_NAME), ("baseline:ip".into(), BASELINE_IP)];
    for (id, hosts) in PROVIDERS {
        for host in *hosts {
            urls.push((id.to_string(), host));
        }
    }
    let results: Vec<(String, Probe)> = std::thread::scope(|s| {
        let handles: Vec<_> = urls.iter().map(|(key, url)| s.spawn(move || (key.clone(), probe(url)))).collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    let find = |key: &str| results.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone()).unwrap_or(Probe { ok: false, reason: "other".into() });
    let mut providers = BTreeMap::new();
    for (id, _) in PROVIDERS {
        // All of a provider's hosts must answer; the first failure is the reason.
        let all: Vec<&Probe> = results.iter().filter(|(k, _)| k == id).map(|(_, p)| p).collect();
        let probe = all.iter().find(|p| !p.ok).map(|p| (*p).clone()).unwrap_or_else(|| all.first().map(|p| (*p).clone()).unwrap_or(Probe { ok: false, reason: "other".into() }));
        providers.insert(id.to_string(), probe);
    }
    (Some(Baseline { by_name: find("baseline:name"), by_ip: find("baseline:ip") }), providers)
}

/// A credential-free `HEAD`: no cookies, no authorization, no body. Redirects are not followed.
pub fn probe(url: &str) -> Probe {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(3)).timeout(Duration::from_secs(5)).redirects(0).user_agent("overseerd-probe").build();
    match agent.head(url).call() {
        Ok(r) => answered(r.status()),
        Err(ureq::Error::Status(code, _)) => answered(code),
        Err(ureq::Error::Transport(t)) => Probe { ok: false, reason: classify_transport(t.kind(), &t.to_string()).into() },
    }
}

fn answered(status: u16) -> Probe {
    Probe { ok: true, reason: format!("answered {status}") }
}

/// The failure class of a transport error, from its kind and its text.
pub fn classify_transport(kind: ureq::ErrorKind, text: &str) -> &'static str {
    let t = text.to_ascii_lowercase();
    if t.contains("certificate") || t.contains("tls") || t.contains("handshake") || t.contains("unknownissuer") {
        "tls"
    } else if kind == ureq::ErrorKind::Dns || t.contains("dns") || t.contains("failed to lookup") || t.contains("nodename nor servname") {
        "dns"
    } else if t.contains("timed out") || t.contains("timeout") {
        "timeout"
    } else if kind == ureq::ErrorKind::ConnectionFailed || t.contains("connection refused") || t.contains("unreachable") || t.contains("network is down") {
        "connect"
    } else {
        "other"
    }
}

/// The system's own answer: is this machine connected to a network with a default route?
pub fn system() -> SystemAnswer {
    if let Some(v) = fixture_file() {
        let state = match v["system"].as_str() {
            Some("connected") => SystemNet::Connected,
            Some("none") => SystemNet::NoNetwork,
            _ => SystemNet::Unknown,
        };
        return SystemAnswer { state, detail: format!("fixture: {}", v["system"].as_str().unwrap_or("unknown")) };
    }
    platform::system()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{SystemAnswer, SystemNet};
    use std::ffi::c_void;

    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn SCNetworkReachabilityCreateWithAddress(allocator: *const c_void, address: *const libc::sockaddr) -> *const c_void;
        fn SCNetworkReachabilityGetFlags(target: *const c_void, flags: *mut u32) -> u8;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    const REACHABLE: u32 = 1 << 1;
    const CONNECTION_REQUIRED: u32 = 1 << 2;
    const CONNECTION_ON_TRAFFIC: u32 = 1 << 3;
    const INTERVENTION_REQUIRED: u32 = 1 << 4;
    const CONNECTION_ON_DEMAND: u32 = 1 << 5;

    fn flags_for(address: *const libc::sockaddr) -> Option<u32> {
        let target = unsafe { SCNetworkReachabilityCreateWithAddress(std::ptr::null(), address) };
        if target.is_null() {
            return None;
        }
        let mut flags = 0u32;
        let ok = unsafe { SCNetworkReachabilityGetFlags(target, &mut flags) };
        unsafe { CFRelease(target) };
        (ok != 0).then_some(flags)
    }

    /// Reachable, and no connection has to be set up by the user first.
    pub fn connected(flags: u32) -> bool {
        if flags & REACHABLE == 0 {
            return false;
        }
        if flags & CONNECTION_REQUIRED == 0 {
            return true;
        }
        flags & (CONNECTION_ON_TRAFFIC | CONNECTION_ON_DEMAND) != 0 && flags & INTERVENTION_REQUIRED == 0
    }

    /// Reachability of the default route, for IPv4 and IPv6 (the zero address of each family).
    pub fn system() -> SystemAnswer {
        let mut v4: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        v4.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
        v4.sin_family = libc::AF_INET as u8;
        let mut v6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
        v6.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
        v6.sin6_family = libc::AF_INET6 as u8;
        let f4 = flags_for(&v4 as *const _ as *const libc::sockaddr);
        let f6 = flags_for(&v6 as *const _ as *const libc::sockaddr);
        let detail = format!("SystemConfiguration reachability: IPv4 {}, IPv6 {}", describe(f4), describe(f6));
        let state = match (f4, f6) {
            (None, None) => SystemNet::Unknown,
            (a, b) if a.is_some_and(connected) || b.is_some_and(connected) => SystemNet::Connected,
            _ => SystemNet::NoNetwork,
        };
        SystemAnswer { state, detail }
    }

    fn describe(flags: Option<u32>) -> String {
        match flags {
            None => "unknown".into(),
            Some(f) => format!("{} (flags 0x{f:08x})", if connected(f) { "reachable" } else { "not reachable" }),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{SystemAnswer, SystemNet};

    pub fn system() -> SystemAnswer {
        if let Ok(out) = std::process::Command::new("nmcli").args(["-t", "-f", "CONNECTIVITY", "general"]).output() {
            if out.status.success() {
                let word = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Some(state) = super::parse_nmcli(&word) {
                    return SystemAnswer { state, detail: format!("NetworkManager connectivity: {word}") };
                }
            }
        }
        let v4 = std::fs::read_to_string("/proc/net/route").map(|t| super::has_default_route(&t)).unwrap_or(false);
        let v6 = std::fs::read_to_string("/proc/net/ipv6_route").map(|t| super::has_default_route_v6(&t)).unwrap_or(false);
        SystemAnswer { state: if v4 || v6 { SystemNet::Connected } else { SystemNet::NoNetwork }, detail: format!("default route: IPv4 {}, IPv6 {}", if v4 { "yes" } else { "no" }, if v6 { "yes" } else { "no" }) }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::{SystemAnswer, SystemNet};
    pub fn system() -> SystemAnswer {
        SystemAnswer { state: SystemNet::Unknown, detail: "the system cannot be asked on this platform yet".into() }
    }
}

/// NetworkManager's connectivity word. `limited` and `portal` still mean a network is there; the
/// probes decide whether it works. `unknown` is left to the route check.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_nmcli(word: &str) -> Option<SystemNet> {
    match word {
        "full" | "limited" | "portal" => Some(SystemNet::Connected),
        "none" => Some(SystemNet::NoNetwork),
        _ => None,
    }
}

/// A default route in `/proc/net/route`: destination and mask both zero, interface up (flag 0x1).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn has_default_route(table: &str) -> bool {
    table.lines().skip(1).any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 7 && f[1] == "00000000" && f[7] == "00000000" && u32::from_str_radix(f[3], 16).is_ok_and(|flags| flags & 0x1 != 0)
    })
}

/// A default route in `/proc/net/ipv6_route`: an all-zero destination with prefix length 00 that
/// is not the loopback device.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn has_default_route_v6(table: &str) -> bool {
    table.lines().any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 9 && f[0].chars().all(|c| c == '0') && f[1] == "00" && f[9] != "lo"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_errors_are_classified() {
        use ureq::ErrorKind::*;
        assert_eq!(classify_transport(Dns, "Dns Failed: resolve dns name 'chatgpt.com:443': failed to lookup address information"), "dns");
        assert_eq!(classify_transport(ConnectionFailed, "Connection Failed: Connect error: Connection refused (os error 61)"), "connect");
        assert_eq!(classify_transport(ConnectionFailed, "Connection Failed: tls connection init failed: invalid peer certificate: UnknownIssuer"), "tls");
        assert_eq!(classify_transport(Io, "Network Error: timed out reading response"), "timeout");
        assert_eq!(classify_transport(Io, "something else"), "other");
    }

    #[test]
    fn the_baseline_names_its_failure() {
        let p = |ok: bool, reason: &str| Probe { ok, reason: reason.into() };
        assert!(Baseline { by_name: p(true, "answered 204"), by_ip: p(false, "connect") }.ok());
        assert_eq!(Baseline { by_name: p(false, "dns"), by_ip: p(true, "answered 301") }.failure(), "DNS is not answering");
        assert_eq!(Baseline { by_name: p(false, "connect"), by_ip: p(false, "connect") }.failure(), "no route to the internet");
        assert_eq!(Baseline { by_name: p(false, "tls"), by_ip: p(false, "tls") }.failure(), "captive portal");
    }

    #[test]
    fn linux_answers_are_parsed() {
        assert_eq!(parse_nmcli("full"), Some(SystemNet::Connected));
        assert_eq!(parse_nmcli("portal"), Some(SystemNet::Connected));
        assert_eq!(parse_nmcli("none"), Some(SystemNet::NoNetwork));
        assert_eq!(parse_nmcli("unknown"), None);
        let up = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\nwlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\nwlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n";
        let local_only = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\nwlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n";
        assert!(has_default_route(up));
        assert!(!has_default_route(local_only));
        assert!(has_default_route_v6("00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000064 00000001 00000000 00000003 wlan0\n"));
        assert!(!has_default_route_v6("00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n"));
    }

    #[test]
    fn a_fixture_stands_in_for_the_network() {
        // The variable is process-wide; this is the only test in this module that sets it, and
        // the macOS test below asks the platform directly.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("net.json");
        std::env::set_var("OVERSEER_TEST_NET", &f);
        std::fs::write(&f, r#"{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": "connect", "anthropic": true}}"#).unwrap();
        let r = read(true);
        assert_eq!(r.system.state, SystemNet::Connected);
        assert!(r.baseline.as_ref().unwrap().ok());
        assert_eq!(r.providers["openai"], Probe { ok: false, reason: "connect".into() });
        assert!(r.providers["anthropic"].ok);
        assert!(read(false).providers.is_empty(), "probes off: only the system is asked");
        std::fs::write(&f, r#"{"system": "none"}"#).unwrap();
        let r = read(true);
        assert_eq!(r.system.state, SystemNet::NoNetwork);
        assert!(r.baseline.is_none(), "no probe is needed when the system says there is no network");
        std::env::remove_var("OVERSEER_TEST_NET");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_reachability_flags() {
        use platform::connected;
        assert!(connected(0x2), "reachable");
        assert!(!connected(0x0), "not reachable");
        assert!(!connected(0x2 | 0x4), "reachable only after the user sets up a connection");
        assert!(connected(0x2 | 0x4 | 0x8), "a connection that comes up on traffic");
        assert!(!connected(0x2 | 0x4 | 0x8 | 0x10), "needs the user's intervention");
        // The real system gives an answer, whichever it is, with the flags it was decided from.
        let answer = platform::system();
        assert_ne!(answer.state, SystemNet::Unknown, "{}", answer.detail);
        assert!(answer.detail.contains("flags 0x"), "{}", answer.detail);
    }
}
