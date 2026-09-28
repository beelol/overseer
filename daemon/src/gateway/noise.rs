//! Noise handshake and transport for the phone gateway (docs/rfcs/phone-remote-protocol.md).
//! Sessions use IK; pairing uses IKpsk1 with the pairing secret. The device is the initiator.

use anyhow::{anyhow, bail, Result};
use sha2::{Digest, Sha256};
use snow::{Builder, HandshakeState, TransportState};

pub const PROLOGUE: &[u8] = b"overseer-gateway-v1";
pub const PATTERN_SESSION: &str = "Noise_IK_25519_ChaChaPoly_SHA256";
/// The secret is mixed into the FIRST message (psk1), so the gateway knows the phone holds the
/// pairing code before it shows anything to the owner. With psk2 a phone without the secret
/// would still reach the confirmation.
pub const PATTERN_PAIRING: &str = "Noise_IKpsk1_25519_ChaChaPoly_SHA256";
/// Where the pre-shared key enters the pairing handshake.
pub const PSK_AT: u8 = 1;
/// Largest plaintext chunk in one transport frame (a Noise message is at most 65,535 bytes).
pub const CHUNK: usize = 65_000;
pub const FRAME_VERSION: u8 = 0x01;
pub const KIND_SESSION: u8 = 0x01;
pub const KIND_PAIRING: u8 = 0x02;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Session,
    Pairing,
}

impl Kind {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            KIND_SESSION => Some(Kind::Session),
            KIND_PAIRING => Some(Kind::Pairing),
            _ => None,
        }
    }
}

pub struct Keypair {
    pub private: Vec<u8>,
    pub public: Vec<u8>,
}

pub fn generate_keypair() -> Result<Keypair> {
    let kp = Builder::new(PATTERN_SESSION.parse()?).generate_keypair()?;
    Ok(Keypair { private: kp.private, public: kp.public })
}

/// The first 16 hex characters of SHA-256 over a public key.
pub fn fingerprint(public: &[u8]) -> String {
    hex(&Sha256::digest(public))[..16].to_string()
}

pub fn psk_from_secret(secret: &[u8]) -> [u8; 32] {
    Sha256::digest(secret).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("odd hex length");
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| anyhow!("bad hex"))).collect()
}

/// The gateway's side of a handshake.
pub fn responder(kind: Kind, static_private: &[u8], psk: Option<&[u8; 32]>) -> Result<HandshakeState> {
    let builder = match kind {
        Kind::Session => Builder::new(PATTERN_SESSION.parse()?),
        Kind::Pairing => Builder::new(PATTERN_PAIRING.parse()?).psk(PSK_AT, psk.ok_or_else(|| anyhow!("pairing needs a secret"))?)?,
    };
    Ok(builder.prologue(PROLOGUE)?.local_private_key(static_private)?.build_responder()?)
}

/// The device's side; used by tests and by the reference client in the protocol tests.
#[allow(dead_code)]
pub fn initiator(kind: Kind, static_private: &[u8], gateway_public: &[u8], psk: Option<&[u8; 32]>) -> Result<HandshakeState> {
    let builder = match kind {
        Kind::Session => Builder::new(PATTERN_SESSION.parse()?),
        Kind::Pairing => Builder::new(PATTERN_PAIRING.parse()?).psk(PSK_AT, psk.ok_or_else(|| anyhow!("pairing needs a secret"))?)?,
    };
    Ok(builder.prologue(PROLOGUE)?.local_private_key(static_private)?.remote_public_key(gateway_public)?.build_initiator()?)
}

/// Splits one protocol message into encrypted transport frames.
pub fn seal(transport: &mut TransportState, message: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut frames = Vec::new();
    let mut chunks = message.chunks(CHUNK).peekable();
    if chunks.peek().is_none() {
        let mut out = vec![0u8; 1 + 16];
        let n = transport.write_message(&[0x01], &mut out)?;
        out.truncate(n);
        frames.push(out);
        return Ok(frames);
    }
    while let Some(chunk) = chunks.next() {
        let mut plain = Vec::with_capacity(chunk.len() + 1);
        plain.push(if chunks.peek().is_some() { 0x00 } else { 0x01 });
        plain.extend_from_slice(chunk);
        let mut out = vec![0u8; plain.len() + 16];
        let n = transport.write_message(&plain, &mut out)?;
        out.truncate(n);
        frames.push(out);
    }
    Ok(frames)
}

/// Joins decrypted chunks into messages. `limit` bounds a joined message.
pub struct Opener {
    pending: Vec<u8>,
    limit: usize,
}

impl Opener {
    pub fn new(limit: usize) -> Self {
        Self { pending: Vec::new(), limit }
    }

    /// Returns a whole message when `frame` was its last chunk.
    pub fn open(&mut self, transport: &mut TransportState, frame: &[u8]) -> Result<Option<Vec<u8>>> {
        if frame.len() < 17 || frame.len() > 65_535 {
            bail!("bad frame length");
        }
        let mut plain = vec![0u8; frame.len()];
        let n = transport.read_message(frame, &mut plain)?;
        plain.truncate(n);
        let (flag, chunk) = plain.split_first().ok_or_else(|| anyhow!("empty frame"))?;
        if chunk.len() > CHUNK {
            bail!("chunk too large");
        }
        if self.pending.len() + chunk.len() > self.limit {
            bail!("message too large");
        }
        self.pending.extend_from_slice(chunk);
        match flag {
            0x00 => Ok(None),
            0x01 => Ok(Some(std::mem::take(&mut self.pending))),
            _ => bail!("bad chunk flag"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixed(seed: u8) -> Vec<u8> {
        (0..32).map(|i| seed.wrapping_add(i as u8).wrapping_mul(7).wrapping_add(3)).collect()
    }

    fn public_of(private: &[u8]) -> Vec<u8> {
        // X25519 base point multiplication through a throwaway NN-style handshake is not exposed;
        // derive it with the same curve implementation snow uses.
        use snow::resolvers::{CryptoResolver, DefaultResolver};
        let mut dh = DefaultResolver.resolve_dh(&snow::params::DHChoice::Curve25519).unwrap();
        dh.set(private);
        dh.pubkey().to_vec()
    }

    /// Deterministic vectors shared with the app (`phone/core`). Regenerate with
    /// `OVERSEER_WRITE_VECTORS=1 cargo test -p overseerd noise_vectors`.
    fn vectors() -> Value {
        let mut out = Vec::new();
        for (name, kind, pattern) in [("session", Kind::Session, PATTERN_SESSION), ("pairing", Kind::Pairing, PATTERN_PAIRING)] {
            let (is, ie, rs, re) = (fixed(1), fixed(2), fixed(3), fixed(4));
            let secret: Vec<u8> = (0..16).map(|i| 0xA0 + i as u8).collect();
            let psk = psk_from_secret(&secret);
            let rs_pub = public_of(&rs);
            let mut ib = Builder::new(pattern.parse().unwrap()).prologue(PROLOGUE).unwrap().local_private_key(&is).unwrap().remote_public_key(&rs_pub).unwrap().fixed_ephemeral_key_for_testing_only(&ie);
            let mut rb = Builder::new(pattern.parse().unwrap()).prologue(PROLOGUE).unwrap().local_private_key(&rs).unwrap().fixed_ephemeral_key_for_testing_only(&re);
            if kind == Kind::Pairing {
                ib = ib.psk(PSK_AT, &psk).unwrap();
                rb = rb.psk(PSK_AT, &psk).unwrap();
            }
            let mut i = ib.build_initiator().unwrap();
            let mut r = rb.build_responder().unwrap();
            let payload1 = br#"{"device":"d-1","name":"Test Phone","platform":"ios","app":"0.1.0","counter":1790000000000}"#;
            let payload2 = format!(r#"{{"protocol":1,"device":"d-1","scope":"full","gateway":"Test Mac","fingerprint":"{}"}}"#, fingerprint(&rs_pub));
            let payload2 = payload2.as_bytes();
            let mut m1 = vec![0u8; 1024];
            let n = i.write_message(payload1, &mut m1).unwrap();
            m1.truncate(n);
            let mut got = vec![0u8; 1024];
            let n = r.read_message(&m1, &mut got).unwrap();
            assert_eq!(&got[..n], payload1);
            assert_eq!(r.get_remote_static().unwrap(), public_of(&is).as_slice());
            let mut m2 = vec![0u8; 1024];
            let n = r.write_message(payload2, &mut m2).unwrap();
            m2.truncate(n);
            let n = i.read_message(&m2, &mut got).unwrap();
            assert_eq!(&got[..n], payload2);
            let hash = hex(i.get_handshake_hash());
            let mut it = i.into_transport_mode().unwrap();
            let mut rt = r.into_transport_mode().unwrap();
            let mut transport = Vec::new();
            for (dir, text) in [("device", r#"{"id":1,"method":"hello","params":{"client":"phone"}}"#), ("gateway", r#"{"id":1,"result":{"protocol":1}}"#), ("device", r#"{"id":2,"method":"state","params":{}}"#), ("gateway", r#"{"method":"event","params":{"seq":1}}"#)] {
                let (tx, rx) = if dir == "device" { (&mut it, &mut rt) } else { (&mut rt, &mut it) };
                let frames = seal(tx, text.as_bytes()).unwrap();
                let mut opener = Opener::new(1 << 20);
                let mut joined = None;
                for f in &frames {
                    joined = opener.open(rx, f).unwrap();
                }
                assert_eq!(joined.unwrap(), text.as_bytes());
                transport.push(json!({"from": dir, "plaintext": text, "frames": frames.iter().map(|f| hex(f)).collect::<Vec<_>>()}));
            }
            out.push(json!({
                "name": name, "pattern": pattern, "prologue": String::from_utf8_lossy(PROLOGUE),
                "initiator_static_private": hex(&is), "initiator_static_public": hex(&public_of(&is)),
                "initiator_ephemeral_private": hex(&ie), "initiator_ephemeral_public": hex(&public_of(&ie)),
                "responder_static_private": hex(&rs), "responder_static_public": hex(&rs_pub),
                "responder_ephemeral_private": hex(&re), "responder_ephemeral_public": hex(&public_of(&re)),
                "pairing_secret": if kind == Kind::Pairing { json!(hex(&secret)) } else { Value::Null },
                "psk": if kind == Kind::Pairing { json!(hex(&psk)) } else { Value::Null },
                "payload1": String::from_utf8_lossy(payload1), "payload2": String::from_utf8_lossy(payload2),
                "message1": hex(&m1), "message2": hex(&m2), "handshake_hash": hash,
                "responder_fingerprint": fingerprint(&rs_pub),
                "transport": transport,
            }));
        }
        json!({"spec": "docs/rfcs/phone-remote-protocol.md", "chunk": CHUNK, "vectors": out})
    }

    #[test]
    fn noise_vectors_match_the_shared_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../protocol/vectors/noise.json");
        let text = serde_json::to_string_pretty(&vectors()).unwrap() + "\n";
        if std::env::var_os("OVERSEER_WRITE_VECTORS").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let on_disk = std::fs::read_to_string(&path).expect("protocol/vectors/noise.json (regenerate with OVERSEER_WRITE_VECTORS=1)");
        assert_eq!(on_disk, text, "the shared Noise vectors changed; the app's implementation must change with them");
    }

    #[test]
    fn a_wrong_pairing_secret_fails_in_the_first_message() {
        let (is, rs) = (fixed(21), fixed(22));
        let right = psk_from_secret(b"the pairing code");
        let wrong = psk_from_secret(b"a guess");
        let mut phone = initiator(Kind::Pairing, &is, &public_of(&rs), Some(&wrong)).unwrap();
        let mut gateway = responder(Kind::Pairing, &rs, Some(&right)).unwrap();
        let mut m1 = vec![0u8; 1024];
        let n = phone.write_message(b"{}", &mut m1).unwrap();
        let mut out = vec![0u8; 1024];
        assert!(gateway.read_message(&m1[..n], &mut out).is_err(), "the gateway must refuse before it asks the owner");
        let mut phone = initiator(Kind::Pairing, &is, &public_of(&rs), Some(&right)).unwrap();
        let mut gateway = responder(Kind::Pairing, &rs, Some(&right)).unwrap();
        let n = phone.write_message(b"{}", &mut m1).unwrap();
        assert!(gateway.read_message(&m1[..n], &mut out).is_ok());
    }

    #[test]
    fn large_messages_are_chunked_and_joined() {
        let (is, rs) = (fixed(9), fixed(10));
        let mut i = initiator(Kind::Session, &is, &public_of(&rs), None).unwrap();
        let mut r = responder(Kind::Session, &rs, None).unwrap();
        let mut buf = vec![0u8; 1024];
        let mut tmp = vec![0u8; 1024];
        let n = i.write_message(b"{}", &mut buf).unwrap();
        r.read_message(&buf[..n], &mut tmp).unwrap();
        let n = r.write_message(b"{}", &mut buf).unwrap();
        i.read_message(&buf[..n], &mut tmp).unwrap();
        let (mut it, mut rt) = (i.into_transport_mode().unwrap(), r.into_transport_mode().unwrap());
        let message: Vec<u8> = (0..200_000).map(|n| (n % 251) as u8).collect();
        let frames = seal(&mut it, &message).unwrap();
        assert_eq!(frames.len(), 4);
        let mut opener = Opener::new(1 << 20);
        let mut joined = None;
        for f in &frames {
            joined = opener.open(&mut rt, f).unwrap();
        }
        assert_eq!(joined.unwrap(), message);
        // Tampered, replayed and reordered frames end the session (the caller closes on Err).
        let frames = seal(&mut it, b"one").unwrap();
        let mut bad = frames[0].clone();
        bad[3] ^= 1;
        assert!(Opener::new(64).open(&mut rt, &bad).is_err());
        assert!(Opener::new(64).open(&mut rt, &frames[0]).is_ok());
        assert!(Opener::new(64).open(&mut rt, &frames[0]).is_err(), "a replayed frame must not decrypt");
        // Over the limit.
        let frames = seal(&mut it, &vec![1u8; 100]).unwrap();
        assert!(Opener::new(64).open(&mut rt, &frames[0]).is_err());
    }
}
