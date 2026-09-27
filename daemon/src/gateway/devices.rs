//! Paired devices and the outcomes of their requests, in the daemon's SQLite store.

use crate::store::Store;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;

/// Stored outcomes are kept this long (the criterion asks for at least 24 hours).
pub const REQUEST_RETENTION_MS: i64 = 7 * 24 * 3600 * 1000;

#[derive(Serialize, Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// Hex of the device's static public key. Never sent to another device.
    pub public_key: String,
    pub scope: String,
    pub paired_ms: i64,
    pub last_seen_ms: Option<i64>,
    pub last_addr: Option<String>,
    #[serde(skip)]
    pub last_counter: i64,
    pub revoked_ms: Option<i64>,
    /// Notification switches and the push token: `{"enabled", "kinds": {..}, "token", "environment"}`.
    pub notifications: Value,
    pub app: Option<String>,
}

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS devices(
          id TEXT PRIMARY KEY, name TEXT NOT NULL, platform TEXT NOT NULL, public_key TEXT NOT NULL UNIQUE,
          scope TEXT NOT NULL, paired_ms INTEGER NOT NULL, last_seen_ms INTEGER, last_addr TEXT,
          last_counter INTEGER NOT NULL DEFAULT 0, revoked_ms INTEGER, notifications TEXT, app TEXT);
        CREATE TABLE IF NOT EXISTS remote_requests(
          device_id TEXT NOT NULL, request_id TEXT NOT NULL, method TEXT NOT NULL, reply TEXT,
          created_ms INTEGER NOT NULL, PRIMARY KEY(device_id, request_id));
        CREATE INDEX IF NOT EXISTS remote_requests_age ON remote_requests(created_ms);
        "#,
    )?;
    Ok(())
}

fn map(row: &Row) -> rusqlite::Result<Device> {
    let notifications: Option<String> = row.get("notifications")?;
    Ok(Device {
        id: row.get("id")?,
        name: row.get("name")?,
        platform: row.get("platform")?,
        public_key: row.get("public_key")?,
        scope: row.get("scope")?,
        paired_ms: row.get("paired_ms")?,
        last_seen_ms: row.get("last_seen_ms")?,
        last_addr: row.get("last_addr")?,
        last_counter: row.get("last_counter")?,
        revoked_ms: row.get("revoked_ms")?,
        notifications: notifications.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null),
        app: row.get("app")?,
    })
}

/// What a stored request says about an earlier attempt.
pub enum Earlier {
    /// Never seen: this attempt may run.
    New,
    /// It ran and this was its reply.
    Done(Value),
    /// It started and the daemon stopped before the outcome was stored.
    Interrupted,
}

impl Store {
    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0)).optional()?)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO meta(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
        Ok(())
    }

    pub fn insert_device(&self, d: &Device) -> Result<()> {
        self.conn.execute(
            "INSERT INTO devices(id,name,platform,public_key,scope,paired_ms,last_seen_ms,last_addr,last_counter,notifications,app) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![d.id, d.name, d.platform, d.public_key, d.scope, d.paired_ms, d.last_seen_ms, d.last_addr, d.last_counter, d.notifications.to_string(), d.app],
        )?;
        Ok(())
    }

    pub fn devices(&self) -> Result<Vec<Device>> {
        let mut stmt = self.conn.prepare("SELECT * FROM devices ORDER BY paired_ms")?;
        let rows = stmt.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn device(&self, id: &str) -> Result<Option<Device>> {
        Ok(self.conn.query_row("SELECT * FROM devices WHERE id=?1", params![id], map).optional()?)
    }

    pub fn device_by_key(&self, public_key_hex: &str) -> Result<Option<Device>> {
        Ok(self.conn.query_row("SELECT * FROM devices WHERE public_key=?1", params![public_key_hex], map).optional()?)
    }

    /// Accepts a handshake counter only when it is higher than every earlier one.
    /// One statement, so two handshakes with the same counter cannot both pass.
    pub fn device_accept_counter(&self, id: &str, counter: i64, now: i64, addr: &str, app: Option<&str>) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE devices SET last_counter=?2, last_seen_ms=?3, last_addr=?4, app=COALESCE(?5, app) WHERE id=?1 AND revoked_ms IS NULL AND last_counter<?2",
            params![id, counter, now, addr, app],
        )?;
        Ok(changed == 1)
    }

    pub fn device_touch(&self, id: &str, now: i64) -> Result<()> {
        self.conn.execute("UPDATE devices SET last_seen_ms=?2 WHERE id=?1", params![id, now])?;
        Ok(())
    }

    pub fn device_revoke(&self, id: &str, now: i64) -> Result<bool> {
        Ok(self.conn.execute("UPDATE devices SET revoked_ms=?2 WHERE id=?1 AND revoked_ms IS NULL", params![id, now])? == 1)
    }

    pub fn device_set_scope(&self, id: &str, scope: &str) -> Result<bool> {
        Ok(self.conn.execute("UPDATE devices SET scope=?2 WHERE id=?1 AND revoked_ms IS NULL", params![id, scope])? == 1)
    }

    pub fn device_rename(&self, id: &str, name: &str) -> Result<bool> {
        Ok(self.conn.execute("UPDATE devices SET name=?2 WHERE id=?1", params![id, name])? == 1)
    }

    pub fn device_set_notifications(&self, id: &str, value: &Value) -> Result<()> {
        self.conn.execute("UPDATE devices SET notifications=?2 WHERE id=?1", params![id, value.to_string()])?;
        Ok(())
    }

    /// Claims a request id. Exactly one caller gets `New` for a given device and id.
    pub fn request_claim(&self, device: &str, request_id: &str, method: &str, now: i64) -> Result<Earlier> {
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO remote_requests(device_id, request_id, method, reply, created_ms) VALUES(?1, ?2, ?3, NULL, ?4)",
            params![device, request_id, method, now],
        )?;
        if inserted == 1 {
            return Ok(Earlier::New);
        }
        let reply: Option<String> = self.conn.query_row("SELECT reply FROM remote_requests WHERE device_id=?1 AND request_id=?2", params![device, request_id], |r| r.get(0))?;
        Ok(match reply.and_then(|t| serde_json::from_str(&t).ok()) {
            Some(v) => Earlier::Done(v),
            None => Earlier::Interrupted,
        })
    }

    pub fn request_store(&self, device: &str, request_id: &str, reply: &Value) -> Result<()> {
        self.conn.execute("UPDATE remote_requests SET reply=?3 WHERE device_id=?1 AND request_id=?2", params![device, request_id, reply.to_string()])?;
        Ok(())
    }

    pub fn request_method(&self, device: &str, request_id: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT method FROM remote_requests WHERE device_id=?1 AND request_id=?2", params![device, request_id], |r| r.get(0)).optional()?)
    }

    pub fn requests_prune(&self, now: i64) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM remote_requests WHERE created_ms<?1", params![now - REQUEST_RETENTION_MS])?)
    }
}
