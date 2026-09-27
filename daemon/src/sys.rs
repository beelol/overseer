//! Platform boundary for memory and disk (Continuity, AC-85): total memory, what is available
//! right now and the system's own pressure signal, read from the operating system. macOS asks the
//! kernel directly; Linux reads `/proc`. Nothing here is tuned to one machine, and what cannot be
//! read is reported as unknown, never as zero.

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

pub const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Pressure {
    Normal,
    Warn,
    Critical,
    Unknown,
}

impl Pressure {
    pub fn parse(s: &str) -> Self {
        match s {
            "normal" => Self::Normal,
            "warn" => Self::Warn,
            "critical" => Self::Critical,
            _ => Self::Unknown,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Memory {
    /// Installed memory in bytes.
    pub total: u64,
    /// Bytes a new process could take now without paging: free, inactive and purgeable pages on
    /// macOS (the numbers `vm_stat` prints); `MemAvailable` on Linux.
    pub available: u64,
    pub pressure: Pressure,
    /// The system's own free-memory percentage where it reports one (macOS `kern.memorystatus_level`).
    pub level: Option<i64>,
    /// Where the numbers came from, for the inventory and the evidence.
    pub source: String,
}

/// The machine's memory now. `OVERSEER_TEST_MEMORY` names a JSON file
/// (`{"total": bytes, "available": bytes, "pressure": "normal|warn|critical"}`) read instead, so
/// tests can describe other machines and squeeze memory; the file is read on every call.
pub fn memory() -> Result<Memory> {
    if let Some(path) = std::env::var_os("OVERSEER_TEST_MEMORY") {
        return fixture(Path::new(&path));
    }
    read()
}

fn fixture(path: &Path) -> Result<Memory> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let total = v["total"].as_u64().ok_or_else(|| anyhow!("memory fixture needs total"))?;
    let available = v["available"].as_u64().ok_or_else(|| anyhow!("memory fixture needs available"))?;
    Ok(Memory { total, available, pressure: Pressure::parse(v["pressure"].as_str().unwrap_or("normal")), level: v["level"].as_i64(), source: format!("fixture {}", path.display()) })
}

#[cfg(target_os = "macos")]
fn sysctl<T: Copy>(name: &str) -> Result<T> {
    let c = std::ffi::CString::new(name)?;
    let mut value: T = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<T>();
    let rc = unsafe { libc::sysctlbyname(c.as_ptr(), &mut value as *mut T as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0) };
    if rc != 0 || len != std::mem::size_of::<T>() {
        bail!("sysctl {name}: {}", std::io::Error::last_os_error());
    }
    Ok(value)
}

#[cfg(target_os = "macos")]
fn read() -> Result<Memory> {
    let total: u64 = sysctl("hw.memsize")?;
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    #[allow(deprecated)]
    let rc = unsafe { libc::host_statistics64(libc::mach_host_self(), libc::HOST_VM_INFO64, &mut stats as *mut _ as *mut libc::integer_t, &mut count) };
    if rc != 0 {
        bail!("host_statistics64 failed with {rc}");
    }
    // `vm_stat` prints "Pages free" without the speculative pages; the same is done here so the
    // numbers can be compared with it.
    let free = (stats.free_count as u64).saturating_sub(stats.speculative_count as u64);
    let available = (free + stats.inactive_count as u64 + stats.purgeable_count as u64) * page;
    let level = sysctl::<i32>("kern.memorystatus_level").ok().map(i64::from);
    let pressure = match sysctl::<i32>("kern.memorystatus_vm_pressure_level") {
        Ok(1) => Pressure::Normal,
        Ok(2) => Pressure::Warn,
        Ok(4) => Pressure::Critical,
        _ => Pressure::Unknown,
    };
    Ok(Memory { total, available, pressure, level, source: "macOS kernel (hw.memsize, host_statistics64, kern.memorystatus_*)".into() })
}

#[cfg(target_os = "linux")]
fn read() -> Result<Memory> {
    let (total, available) = parse_meminfo(&std::fs::read_to_string("/proc/meminfo")?)?;
    let pressure = std::fs::read_to_string("/proc/pressure/memory").map(|t| parse_psi(&t)).unwrap_or(Pressure::Unknown);
    Ok(Memory { total, available, pressure, level: None, source: "Linux /proc/meminfo and /proc/pressure/memory".into() })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read() -> Result<Memory> {
    bail!("memory cannot be read on this platform yet")
}

/// `MemTotal` and `MemAvailable` from `/proc/meminfo`, in bytes.
pub fn parse_meminfo(text: &str) -> Result<(u64, u64)> {
    let field = |name: &str| -> Option<u64> {
        text.lines().find_map(|l| l.strip_prefix(name)).and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok()).map(|kb| kb * 1024)
    };
    match (field("MemTotal:"), field("MemAvailable:")) {
        (Some(t), Some(a)) => Ok((t, a)),
        _ => bail!("/proc/meminfo has no MemTotal or MemAvailable"),
    }
}

/// Pressure from Linux PSI (`/proc/pressure/memory`): the share of the last ten seconds in which
/// all tasks (`full`) or some task (`some`) stalled on memory. 5% or more of `full` is critical,
/// 5% or more of `some` is a warning.
pub fn parse_psi(text: &str) -> Pressure {
    let avg10 = |kind: &str| -> Option<f64> {
        text.lines().find(|l| l.starts_with(kind))?.split_whitespace().find_map(|f| f.strip_prefix("avg10=")).and_then(|v| v.parse().ok())
    };
    match (avg10("some"), avg10("full")) {
        (_, Some(full)) if full >= 5.0 => Pressure::Critical,
        (Some(some), _) if some >= 5.0 => Pressure::Warn,
        (Some(_), _) => Pressure::Normal,
        _ => Pressure::Unknown,
    }
}

/// Free bytes on the filesystem holding `path` (its nearest existing parent), for downloads.
pub fn disk_free(path: &Path) -> Option<u64> {
    let mut at = path;
    while !at.exists() {
        at = at.parent()?;
    }
    let c = std::ffi::CString::new(at.as_os_str().to_string_lossy().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    Some(s.f_bavail as u64 * s.f_frsize as u64)
}

pub fn gib(bytes: u64) -> f64 {
    (bytes as f64 / GIB as f64 * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_meminfo_is_read_in_bytes() {
        let text = "MemTotal:       32658040 kB\nMemFree:         1200000 kB\nMemAvailable:   20123456 kB\nBuffers:          100000 kB\n";
        let (total, available) = parse_meminfo(text).unwrap();
        assert_eq!(total, 32658040 * 1024);
        assert_eq!(available, 20123456 * 1024);
        assert!(parse_meminfo("MemTotal: 1 kB\n").is_err(), "a missing MemAvailable is an error, not zero");
    }

    #[test]
    fn linux_pressure_levels() {
        assert_eq!(parse_psi("some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"), Pressure::Normal);
        assert_eq!(parse_psi("some avg10=7.50 avg60=1.00 avg300=0.10 total=9\nfull avg10=1.00 avg60=0.00 avg300=0.00 total=1\n"), Pressure::Warn);
        assert_eq!(parse_psi("some avg10=40.00 avg60=9.00 avg300=1.00 total=9\nfull avg10=12.00 avg60=2.00 avg300=0.00 total=1\n"), Pressure::Critical);
        assert_eq!(parse_psi(""), Pressure::Unknown);
    }

    #[test]
    fn a_fixture_describes_another_machine() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("mem.json");
        std::fs::write(&f, r#"{"total": 34359738368, "available": 17179869184, "pressure": "warn"}"#).unwrap();
        let m = fixture(&f).unwrap();
        assert_eq!((m.total, m.available, m.pressure), (32 * GIB, 16 * GIB, Pressure::Warn));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_numbers_match_the_systems_own_tools() {
        let run = |cmd: &str, args: &[&str]| String::from_utf8(std::process::Command::new(cmd).args(args).output().unwrap().stdout).unwrap();
        let before = read().unwrap();
        let vm = run("/usr/bin/vm_stat", &[]);
        let after = read().unwrap();
        let total: u64 = run("/usr/sbin/sysctl", &["-n", "hw.memsize"]).trim().parse().unwrap();
        assert_eq!(before.total, total);
        let page: u64 = vm.split("page size of ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        let pages = |name: &str| -> u64 { vm.lines().find(|l| l.starts_with(name)).unwrap().split(':').nth(1).unwrap().trim().trim_end_matches('.').parse().unwrap() };
        let expected = (pages("Pages free") + pages("Pages inactive") + pages("Pages purgeable")) * page;
        // Memory moves between the three readings; the daemon's number must sit within 5% of vm_stat's.
        let ours = (before.available + after.available) / 2;
        let diff = ours.abs_diff(expected) as f64 / expected as f64;
        assert!(diff < 0.05, "available {ours} vs vm_stat {expected} ({:.1}% apart)", diff * 100.0);
        assert!(before.available < before.total);
        assert_ne!(before.pressure, Pressure::Unknown, "the kernel reports a pressure level");
        assert!(before.level.is_some_and(|l| (0..=100).contains(&l)));
    }

    #[test]
    fn disk_free_is_reported_for_a_path_that_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        assert!(disk_free(&dir.path().join("models/not/created")).is_some_and(|b| b > 0));
    }
}
