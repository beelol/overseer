use super::error;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

pub const MANIFEST_LIMIT: usize = 64 * 1024;
pub const TEXT_LIMIT: usize = 32 * 1024;
pub const BUNDLE_LIMIT: usize = 1024 * 1024;
pub const FILE_LIMIT: usize = 128;
pub const DELIVERY_LIMIT: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub files: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub file: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub source: String,
    pub homepage: Option<String>,
    pub rules: Option<Rules>,
    pub style: Option<Style>,
    pub tools: Option<toml::Value>,
    pub hooks: Option<toml::Value>,
    pub program: Option<toml::Value>,
    pub programs: Option<toml::Value>,
    pub limits: Option<toml::Value>,
    pub harness_options: Option<toml::Value>,
    pub skills: Option<toml::Value>,
}

pub fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\\')
        || !Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(error(
            "invalid_mod",
            "mod paths must be relative and cannot escape the bundle",
        ));
    }
    Ok(())
}
pub fn parse(bytes: &[u8]) -> Result<Manifest> {
    if bytes.len() > MANIFEST_LIMIT {
        return Err(error("invalid_mod", "manifest exceeds 64 KiB"));
    }
    let s =
        std::str::from_utf8(bytes).map_err(|_| error("invalid_mod", "manifest must be UTF-8"))?;
    let m: Manifest = toml::from_str(s)
        .map_err(|e| error("invalid_mod", format!("invalid mod manifest: {e}")))?;
    if m.schema_version != 1 {
        return Err(error("unsupported_mod", "unsupported mod schema version"));
    }
    if m.id.is_empty()
        || m.id.len() > 64
        || !m
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(error(
            "invalid_mod",
            "id must contain 1–64 lower-case letters, digits or hyphens",
        ));
    }
    if m.version.trim().is_empty() || m.version.len() > 32 || m.name.trim().is_empty() {
        return Err(error(
            "invalid_mod",
            "name and a 1–32 byte version are required",
        ));
    }
    if m.tools.is_some()
        || m.hooks.is_some()
        || m.program.is_some()
        || m.programs.is_some()
        || m.limits.is_some()
        || m.harness_options.is_some()
        || m.skills.is_some()
    {
        return Err(error("unsupported_mod", "tools, hooks, programs, limits, harness options and skills are unsupported in the text-only release"));
    }
    let mut files = m
        .rules
        .as_ref()
        .map(|r| r.files.clone())
        .unwrap_or_default();
    if let Some(style) = &m.style {
        files.push(style.file.clone());
    }
    if files.is_empty() {
        return Err(error("invalid_mod", "a text mod needs rules or style"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for file in files {
        relative(&file)?;
        if file == "mod.toml" || !seen.insert(file) {
            return Err(error(
                "invalid_mod",
                "text files must be distinct from each other and the manifest",
            ));
        }
    }
    Ok(m)
}
