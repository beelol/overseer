//! Pairing: opened on the Mac, one secret, one use, two minutes, and the owner confirms.

use super::{base32, noise};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

pub const WINDOW: Duration = Duration::from_secs(120);
/// How long a phone waits, silent, while the owner decides.
pub const CONFIRM_WAIT: Duration = Duration::from_secs(60);

pub fn window() -> Duration {
    super::test_shorter("OVERSEER_TEST_PAIRING_WINDOW_MS", WINDOW)
}

pub fn confirm_wait() -> Duration {
    super::test_shorter("OVERSEER_TEST_CONFIRM_WAIT_MS", CONFIRM_WAIT)
}
pub const MAX_FAILURES: u32 = 5;

pub struct Pending {
    pub name: String,
    pub platform: String,
    pub addr: String,
    pub fingerprint: String,
    pub decide: oneshot::Sender<bool>,
}

pub struct Pairing {
    pub secret: [u8; 16],
    pub psk: [u8; 32],
    pub opened: Instant,
    pub failures: u32,
    /// A phone presented the secret; no second phone may use it.
    pub used: bool,
    pub pending: HashMap<String, Pending>,
}

impl Pairing {
    pub fn open() -> anyhow::Result<Self> {
        let mut secret = [0u8; 16];
        getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("no random bytes: {e}"))?;
        Ok(Self { psk: noise::psk_from_secret(&secret), secret, opened: Instant::now(), failures: 0, used: false, pending: HashMap::new() })
    }

    pub fn expired(&self) -> bool {
        self.opened.elapsed() > window()
    }

    pub fn accepts_attempts(&self) -> bool {
        !self.expired() && !self.used && self.failures < MAX_FAILURES
    }

    pub fn remaining_ms(&self) -> u64 {
        window().saturating_sub(self.opened.elapsed()).as_millis() as u64
    }
}

/// The text a phone scans or types (docs/rfcs/phone-remote-protocol.md, "Pairing code").
pub fn code(gateway_public: &[u8], secret: &[u8; 16], port: u16, addresses: &[String]) -> String {
    let mut bytes = Vec::with_capacity(64);
    bytes.push(0x01);
    bytes.extend_from_slice(gateway_public);
    bytes.extend_from_slice(secret);
    bytes.extend_from_slice(&port.to_be_bytes());
    let addresses: Vec<&String> = addresses.iter().filter(|a| !a.is_empty() && a.len() < 256 && a.is_ascii()).take(8).collect();
    bytes.push(addresses.len() as u8);
    for a in addresses {
        bytes.push(a.len() as u8);
        bytes.extend_from_slice(a.as_bytes());
    }
    format!("OVSR1-{}", base32::encode(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pairing_code_carries_key_secret_port_and_addresses() {
        let key: Vec<u8> = (0..32).collect();
        let secret = [7u8; 16];
        let text = code(&key, &secret, 47810, &["192.168.1.20".into(), "127.0.0.1".into()]);
        assert!(text.starts_with("OVSR1-"));
        let bytes = base32::decode(&text[6..]).unwrap();
        assert_eq!(bytes[0], 1);
        assert_eq!(&bytes[1..33], key.as_slice());
        assert_eq!(&bytes[33..49], &secret);
        assert_eq!(u16::from_be_bytes([bytes[49], bytes[50]]), 47810);
        assert_eq!(bytes[51], 2);
        assert_eq!(bytes[52] as usize, "192.168.1.20".len());
        assert_eq!(&bytes[53..65], b"192.168.1.20");
        assert_eq!(bytes.len(), 53 + 12 + 1 + 9);
    }

    #[test]
    fn a_pairing_window_closes_on_use_failures_and_time() {
        let mut p = Pairing::open().unwrap();
        assert!(p.accepts_attempts());
        p.failures = MAX_FAILURES;
        assert!(!p.accepts_attempts());
        p.failures = 0;
        p.used = true;
        assert!(!p.accepts_attempts());
        p.used = false;
        p.opened = Instant::now() - WINDOW - Duration::from_secs(1);
        assert!(p.expired() && !p.accepts_attempts());
        assert_ne!(Pairing::open().unwrap().secret, Pairing::open().unwrap().secret);
    }
}
