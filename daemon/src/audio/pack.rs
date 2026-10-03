//! Descriptor-rooted, in-place pack resolution. No media is copied or retained in a cache.
use super::{decode, lines::Line};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::time::{Duration, Instant};

pub(super) const MEDIA_LIMIT: u64 = 8 * 1024 * 1024;
const MANIFEST_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Identity {
    dev: u64,
    ino: u64,
    len: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Identity {
    fn file(file: &File) -> Result<Self> {
        let m = file
            .metadata()
            .map_err(|_| anyhow!("Cannot inspect the selected pack."))?;
        Ok(Self {
            dev: m.dev(),
            ino: m.ino(),
            len: m.len(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        })
    }
    fn same_directory(&self, other: &Self) -> bool {
        self.dev == other.dev && self.ino == other.ino
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ValidatedPack {
    pub(super) id: String,
    pub(super) label: String,
    root: Identity,
    manifest: Identity,
    manifest_digest: String,
    lines: BTreeMap<String, ValidatedLine>,
}
#[derive(Clone, Serialize, Deserialize)]
struct ValidatedLine {
    relative: String,
    identity: Identity,
    duration_ms: u64,
}

pub(super) struct Pack {
    root: File,
    pub(super) validated: ValidatedPack,
}
pub(super) struct OpenedLine {
    pub(super) file: File,
    pub(super) duration_ms: u64,
    pub(super) pack_id: String,
}

fn open_at(parent: &File, name: &str, directory: bool) -> Result<File> {
    let name = CString::new(name).map_err(|_| anyhow!("Invalid pack path."))?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | libc::O_NOFOLLOW
        | if directory { libc::O_DIRECTORY } else { 0 };
    // NONBLOCK precedes fstat: a substituted FIFO must never hold the daemon here.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        bail!("A pack file is missing or unsafe.");
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file
        .metadata()
        .map_err(|_| anyhow!("Cannot inspect a pack file."))?;
    if (directory && !m.is_dir()) || (!directory && !m.is_file()) {
        bail!("A pack path is not a regular file or folder.");
    }
    Ok(file)
}
fn root(path: &Path) -> Result<File> {
    if !path.is_absolute() {
        bail!("Choose an absolute local folder.");
    }
    let mut dir = File::open("/").map_err(|_| anyhow!("Cannot open the selected folder."))?;
    let mut depth = 0;
    for c in path.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(s) => {
                depth += 1;
                if depth > 64 {
                    bail!("The selected folder path is too deep.");
                }
                dir = open_at(
                    &dir,
                    s.to_str().ok_or_else(|| anyhow!("Invalid folder path."))?,
                    true,
                )?;
            }
            _ => bail!("Invalid folder path."),
        }
    }
    Ok(dir)
}
fn relative_parts(relative: &str) -> Result<Vec<&str>> {
    if relative.len() > 1024 || relative.contains('\\') || relative.contains('\0') {
        bail!("Invalid relative audio path.");
    }
    let parts: Vec<_> = relative.split('/').collect();
    if parts.is_empty()
        || parts.len() > 16
        || parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        bail!("Audio paths must stay inside the pack folder.");
    }
    if !matches!(
        Path::new(relative).extension().and_then(|s| s.to_str()),
        Some("wav" | "mp3")
    ) {
        bail!("Pack files must be WAV or MP3 audio.");
    }
    Ok(parts)
}
fn media(root: &File, relative: &str) -> Result<File> {
    let parts = relative_parts(relative)?;
    let mut dir = root
        .try_clone()
        .map_err(|_| anyhow!("Cannot open the pack folder."))?;
    for p in &parts[..parts.len() - 1] {
        dir = open_at(&dir, p, true)?;
    }
    let file = open_at(&dir, parts[parts.len() - 1], false)?;
    if !(1..=MEDIA_LIMIT).contains(
        &file
            .metadata()
            .map_err(|_| anyhow!("Cannot inspect audio."))?
            .len(),
    ) {
        bail!("Each audio file must be nonempty and at most 8 MiB.");
    }
    Ok(file)
}
fn manifest(root: &File) -> Result<(Identity, String, serde_json::Value)> {
    let file = open_at(root, "audio-pack.json", false)?;
    let before = Identity::file(&file)?;
    if before.len > MANIFEST_LIMIT {
        bail!("The pack manifest exceeds 64 KiB.");
    }
    let mut bytes = Vec::new();
    (&file)
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("Cannot read the pack manifest."))?;
    if bytes.len() as u64 > MANIFEST_LIMIT || Identity::file(&file)? != before {
        bail!("The pack manifest changed while being read.");
    }
    let value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("The pack manifest is not valid JSON."))?;
    Ok((before, format!("{:x}", Sha256::digest(&bytes)), value))
}
fn text(value: &serde_json::Value, key: &str, max: usize) -> Result<String> {
    let text = value[key]
        .as_str()
        .ok_or_else(|| anyhow!("The pack needs a valid id and label."))?;
    if text.is_empty() || text.len() > max || text.chars().any(char::is_control) {
        bail!("The pack needs a bounded printable id and label.");
    }
    Ok(text.into())
}
impl Pack {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let root = root(path)?;
        let root_id = Identity::file(&root)?;
        let (manifest_id, digest, value) = manifest(&root)?;
        if value["schema"] != 1 {
            bail!("The pack needs audio-pack.json schema 1.");
        }
        let id = text(&value, "id", 64)?;
        if !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            bail!("The pack id must use letters, numbers, dots, dashes or underscores.");
        }
        let label = text(&value, "label", 120)?;
        let mappings = value["lines"]
            .as_object()
            .ok_or_else(|| anyhow!("The pack needs twelve line mappings."))?;
        if mappings.len() != Line::ALL.len() || mappings.keys().any(|k| Line::parse(k).is_none()) {
            bail!("The pack must contain exactly the twelve approved lines.");
        }
        let deadline = Instant::now() + Duration::from_secs(25);
        let mut lines = BTreeMap::new();
        for line in Line::ALL {
            let relative = mappings
                .get(line.key())
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Every line needs a relative audio path."))?;
            let file = media(&root, relative)?;
            let identity = Identity::file(&file)?;
            let duration_ms = decode::validate(&file, deadline)?;
            if Identity::file(&file)? != identity {
                bail!("An audio file changed during validation.");
            }
            lines.insert(
                line.key().into(),
                ValidatedLine {
                    relative: relative.into(),
                    identity,
                    duration_ms,
                },
            );
        }
        let pack = Self {
            root,
            validated: ValidatedPack {
                id,
                label,
                root: root_id,
                manifest: manifest_id,
                manifest_digest: digest,
                lines,
            },
        };
        // Reopen checks also prove the named tree still refers to the reviewed descriptors.
        Self::from_validated(path, pack.validated.clone())?;
        Ok(pack)
    }
    /// Cheap availability check: bounded manifest read and twelve descriptor stats, never decode.
    pub(super) fn from_validated(path: &Path, validated: ValidatedPack) -> Result<Self> {
        let root = root(path)?;
        if !Identity::file(&root)?.same_directory(&validated.root) {
            bail!("The selected folder was replaced. Select it again.");
        }
        let (id, digest, _) = manifest(&root)?;
        if id != validated.manifest || digest != validated.manifest_digest {
            bail!("The pack manifest changed. Select the folder again.");
        }
        if validated.lines.len() != Line::ALL.len() {
            bail!("The selected pack needs validation.");
        }
        for line in Line::ALL {
            let entry = validated
                .lines
                .get(line.key())
                .ok_or_else(|| anyhow!("The selected pack needs validation."))?;
            if Identity::file(&media(&root, &entry.relative)?)? != entry.identity {
                bail!("A pack file changed. Select the folder again.");
            }
        }
        Ok(Self { root, validated })
    }
    pub(super) fn open_line(&self, line: Line) -> Result<OpenedLine> {
        let entry = self
            .validated
            .lines
            .get(line.key())
            .ok_or_else(|| anyhow!("The pack needs twelve lines."))?;
        let file = media(&self.root, &entry.relative)?;
        if Identity::file(&file)? != entry.identity {
            bail!("The selected audio file changed.");
        }
        let duration_ms = decode::validate(&file, Instant::now() + Duration::from_secs(2))?;
        if Identity::file(&file)? != entry.identity {
            bail!("The audio changed during validation.");
        }
        Ok(OpenedLine {
            file,
            duration_ms,
            pack_id: self.validated.id.clone(),
        })
    }
}
