//! The terminal UI's QR encoder against the encoder VS Code ships: for the same text, level and
//! mask, every module is the same (tests/support/qr-vectors.json, written by
//! test/ui/qr-vectors.js). And what it draws with half blocks reads back as the text.
mod support;

use overseer_tui::qr::{encode, encode_with_mask, Level};
use serde_json::Value;
use support::qr_read;

fn level(name: &str) -> Level {
    match name {
        "L" => Level::L,
        "M" => Level::M,
        "Q" => Level::Q,
        _ => Level::H,
    }
}

#[test]
fn every_module_matches_the_encoder_vs_code_ships() {
    let file: Value = serde_json::from_str(include_str!("support/qr-vectors.json")).unwrap();
    let vectors = file["vectors"].as_array().unwrap();
    assert!(vectors.len() >= 30);
    let mut versions = std::collections::BTreeSet::new();
    for v in vectors {
        let text = v["text"].as_str().unwrap();
        let mask = v["mask"].as_u64().unwrap() as u8;
        let qr = encode_with_mask(text.as_bytes(), level(v["level"].as_str().unwrap()), Some(mask)).unwrap();
        assert_eq!(qr.version as u64, v["version"].as_u64().unwrap(), "{} {}", v["level"], text.len());
        let want: Vec<String> = v["rows"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect();
        let got = qr.rows();
        for (y, (a, b)) in got.iter().zip(&want).enumerate() {
            assert_eq!(a, b, "row {y} of {} {} bytes, mask {mask}", v["level"], text.len());
        }
        assert_eq!(got.len(), want.len());
        versions.insert(qr.version);
    }
    assert!(versions.len() >= 12, "{versions:?}");
}

#[test]
fn the_code_it_chooses_reads_back_at_every_length() {
    let alphabet: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".chars().collect();
    for lvl in [Level::L, Level::M, Level::Q, Level::H] {
        let mut n = 1;
        while n <= 600 {
            let text: String = "OVSR1-".chars().chain((0..).map(|i| alphabet[(i * 11 + n) % 32])).take(n).collect();
            let qr = encode(text.as_bytes(), lvl).unwrap();
            let rows: Vec<Vec<bool>> = (0..qr.size).map(|r| (0..qr.size).map(|c| qr.dark(r, c)).collect()).collect();
            let read = qr_read::decode(&rows).unwrap_or_else(|e| panic!("{lvl:?} {n}: {e}"));
            assert_eq!(read.text, text, "{lvl:?} {n}");
            assert_eq!(read.mask, qr.mask);
            assert_eq!(read.level, format!("{lvl:?}"));
            n += if lvl == Level::M { 1 } else { 17 };
        }
    }
}

#[test]
fn half_blocks_read_back_as_the_text() {
    let text = "OVSR1-AHKKWZKCLJCHKSH6KKYAPUOMUUQGDALLBECJJ63CIT3YOHOYNDESYESQ6RKRXYF5PWRJGVKJP3WEJF7EHABA4MJZGIXDCNRYFY2TALRRHA2ASMJSG4XDALRQFYYQ";
    for quiet in [2, 4] {
        let qr = encode(text.as_bytes(), Level::M).unwrap();
        let lines = qr.half_blocks(quiet);
        let rows = qr_read::from_half_blocks(&lines, quiet).unwrap();
        assert_eq!(rows.len(), qr.size);
        assert_eq!(qr_read::decode(&rows).unwrap().text, text);
    }
    // A damaged module is noticed.
    let qr = encode(text.as_bytes(), Level::M).unwrap();
    let mut rows: Vec<Vec<bool>> = (0..qr.size).map(|r| (0..qr.size).map(|c| qr.dark(r, c)).collect()).collect();
    rows[20][20] ^= true;
    assert!(qr_read::decode(&rows).is_err());
}
