//! Optional text bundles. This module never loads or executes mod code.
pub mod bindings;
pub mod delivery;
pub mod library;
pub mod manifest;
pub(crate) mod read;

use crate::{server::ProtoError, store::Store};
use anyhow::Result;

pub fn error(code: &'static str, message: impl Into<String>) -> anyhow::Error {
    ProtoError::new(code, message).into()
}
pub fn revision(store: &Store) -> Result<i64> {
    Ok(store
        .conn
        .query_row(
            "SELECT value FROM meta WHERE key='mods_revision'",
            [],
            |r| r.get::<_, String>(0),
        )?
        .parse()?)
}
pub fn advance(store: &Store) -> Result<i64> {
    let next = revision(store)? + 1;
    store.conn.execute(
        "UPDATE meta SET value=?1 WHERE key='mods_revision'",
        [next.to_string()],
    )?;
    Ok(next)
}
pub fn expect_revision(store: &Store, p: &serde_json::Value) -> Result<()> {
    let current = revision(store)?;
    if p["expected_revision"].as_i64() != Some(current) {
        return Err(ProtoError::new(
            "revision_conflict",
            "Mods changed; read current state and retry",
        )
        .with_data(serde_json::json!({"revision":current}))
        .into());
    }
    Ok(())
}
pub fn text<'a>(p: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    p[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error("invalid_mod", format!("missing {key}")))
}
