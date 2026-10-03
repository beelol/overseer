use super::{
    advance, error, expect_revision,
    manifest::{self, Manifest},
    revision, text,
};
use crate::{
    daemon::{now, Daemon},
    paths,
    store::Store,
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::{
        fs::OpenOptionsExt,
        io::{AsRawFd, FromRawFd},
    },
    path::Path,
    sync::{Arc, Mutex},
};
static FILE_OPERATIONS: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bundle {
    pub manifest: Manifest,
    pub fingerprint: String,
    pub source: String,
    pub installed_ms: i64,
    pub files: BTreeMap<String, String>,
}
impl Bundle {
    pub fn public(&self) -> Value {
        json!({"id":self.manifest.id,"version":self.manifest.version,"fingerprint":self.fingerprint,
            "manifest":self.manifest,"source":self.source,"installed_ms":self.installed_ms,
            "files":self.files.iter().map(|(path,text)| json!({"path":path,"bytes":text.len(),"sha256":format!("{:x}",Sha256::digest(text.as_bytes()))})).collect::<Vec<_>>()})
    }
    fn from_files(files: BTreeMap<String, String>, source: String) -> Result<Self> {
        if files.len() > manifest::FILE_LIMIT
            || files.values().map(String::len).sum::<usize>() > manifest::BUNDLE_LIMIT
        {
            return Err(error("invalid_mod", "bundle exceeds 128 files or 1 MiB"));
        }
        let manifest = manifest::parse(
            files
                .get("mod.toml")
                .ok_or_else(|| error("invalid_mod", "missing mod.toml"))?
                .as_bytes(),
        )?;
        let mut declared = manifest
            .rules
            .as_ref()
            .map(|r| r.files.clone())
            .unwrap_or_default();
        if let Some(style) = &manifest.style {
            declared.push(style.file.clone());
        }
        for file in declared {
            let contents = files
                .get(&file)
                .ok_or_else(|| error("invalid_mod", format!("missing text file {file}")))?;
            if contents.len() > manifest::TEXT_LIMIT {
                return Err(error("invalid_mod", "text exceeds 32 KiB"));
            }
        }
        let mut hash = Sha256::new();
        for (path, contents) in &files {
            hash.update((path.len() as u64).to_be_bytes());
            hash.update(path.as_bytes());
            hash.update((contents.len() as u64).to_be_bytes());
            hash.update(contents.as_bytes());
        }
        Ok(Self {
            manifest,
            fingerprint: format!("{:x}", hash.finalize()),
            source,
            installed_ms: 0,
            files,
        })
    }
}

pub fn versions(store: &Store) -> Result<Vec<Bundle>> {
    let mut stmt = store
        .conn
        .prepare("SELECT content FROM mod_versions ORDER BY mod_id,fingerprint")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
}

fn bundled() -> Result<Bundle> {
    Bundle::from_files(
        BTreeMap::from([
            (
                "mod.toml".into(),
                include_str!("../../../mods/clear-prose/mod.toml").into(),
            ),
            (
                "style.md".into(),
                include_str!("../../../mods/clear-prose/style.md").into(),
            ),
        ]),
        "bundled:clear-prose".into(),
    )
}

// Every traversal and read is anchored to an already-open directory. No-follow
// openat prevents a source-directory race from importing files outside the bundle.
fn read_tree(
    dir: &File,
    prefix: &str,
    files: &mut BTreeMap<String, String>,
    total: &mut usize,
    visited: &mut usize,
) -> Result<()> {
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let fd = unsafe { libc::dup(dir.as_raw_fd()) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err(std::io::Error::last_os_error().into());
    }
    let entries = Directory(raw);
    loop {
        let entry = unsafe { libc::readdir(entries.0) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        *visited += 1;
        if *visited > 256 {
            return Err(error(
                "invalid_mod",
                "bundle exceeds 256 total files and directories",
            ));
        }
        let name_text = std::str::from_utf8(bytes)
            .map_err(|_| error("invalid_mod", "file names must be UTF-8"))?;
        let path = format!("{prefix}{name_text}");
        manifest::relative(&path)?;
        let child = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if child < 0 {
            return Err(error(
                "invalid_mod",
                format!("cannot import {path}: symlinks and unreadable files are refused"),
            ));
        }
        let file = unsafe { File::from_raw_fd(child) };
        let meta = file.metadata()?;
        if meta.is_dir() {
            // Bound directory depth even for trees containing no regular files.
            if path.split('/').count() > 16 {
                return Err(error("invalid_mod", "bundle directories are too deep"));
            }
            read_tree(&file, &format!("{path}/"), files, total, visited)?;
        } else if meta.is_file() {
            if files.len() >= manifest::FILE_LIMIT {
                return Err(error("invalid_mod", "bundle exceeds 128 files"));
            }
            let limit = if path == "mod.toml" {
                manifest::MANIFEST_LIMIT
            } else {
                manifest::TEXT_LIMIT
            };
            let mut data = Vec::new();
            file.take((limit + 1) as u64).read_to_end(&mut data)?;
            if data.len() > limit {
                return Err(error(
                    "invalid_mod",
                    format!("file {path} exceeds its size limit"),
                ));
            }
            *total += data.len();
            if *total > manifest::BUNDLE_LIMIT {
                return Err(error("invalid_mod", "bundle exceeds 1 MiB"));
            }
            let contents = String::from_utf8(data)
                .map_err(|_| error("invalid_mod", "bundle files must be UTF-8"))?;
            files.insert(path, contents);
        } else {
            return Err(error("invalid_mod", "special files are refused"));
        }
    }
    Ok(())
}

fn import(source: &str) -> Result<Bundle> {
    if source == "bundled:clear-prose" {
        return bundled();
    }
    let path = Path::new(source);
    if !path.is_absolute() {
        return Err(error(
            "invalid_mod",
            "source must be an absolute folder or bundled:clear-prose",
        ));
    }
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(error("invalid_mod", "source cannot be a symlink"));
    }
    let path = fs::canonicalize(path)?;
    // Open every source component with no-follow, including ancestor directories.
    let mut dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for component in path.components().skip(1) {
        let std::path::Component::Normal(name) = component else {
            return Err(error("invalid_mod", "source path has invalid components"));
        };
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(name.as_bytes())?;
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(error(
                "invalid_mod",
                "source folders cannot contain symlinks",
            ));
        }
        dir = unsafe { File::from_raw_fd(fd) };
    }
    let mut files = BTreeMap::new();
    let mut total = 0;
    read_tree(&dir, "", &mut files, &mut total, &mut 0)?;
    Bundle::from_files(files, source.into())
}

fn write_tree(root: &Path, bundle: &Bundle) -> Result<()> {
    paths::ensure_private_dir(root)?;
    for (path, contents) in &bundle.files {
        let target = root.join(path);
        paths::ensure_private_dir(target.parent().unwrap())?;
        use std::io::Write;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(target)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    Ok(())
}

fn discard(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub fn recover(store: &Store) -> Result<()> {
    let stage = paths::mods_dir().join("previews");
    if stage.exists() {
        for entry in fs::read_dir(stage)? {
            let entry = entry?;
            let id = entry
                .file_name()
                .into_string()
                .map_err(|_| error("invalid_mod", "invalid staged file name"))?;
            let pending = store
                .conn
                .prepare("SELECT 1 FROM mod_previews WHERE id=?1 AND result IS NULL")?
                .exists([id])?;
            if !pending {
                discard(&entry.path())?;
            }
        }
    }
    let installed = versions(store)?;
    let root = paths::mods_dir().join("versions");
    if root.exists() {
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                discard(&entry.path())?;
                continue;
            }
            for version in fs::read_dir(entry.path())? {
                let version = version?;
                let known = installed.iter().any(|v| {
                    entry.file_name() == v.manifest.id.as_str()
                        && version.file_name() == v.fingerprint.as_str()
                });
                if !known {
                    discard(&version.path())?;
                }
            }
        }
    }
    for bundle in installed {
        let target = root.join(&bundle.manifest.id).join(&bundle.fingerprint);
        // Database copies are authoritative. Rebuild an incomplete or changed tree.
        let intact = bundle.files.iter().all(|(path, text)| {
            fs::symlink_metadata(target.join(path)).is_ok_and(|m| m.is_file())
                && fs::read(target.join(path)).is_ok_and(|bytes| bytes == text.as_bytes())
        });
        if !intact {
            discard(&target)?;
            write_tree(&target, &bundle)?;
        }
    }
    Ok(())
}
pub fn list(d: &Daemon) -> Result<Value> {
    let store = d.store.lock().unwrap();
    let versions = versions(&store)?;
    let mut stmt = store
        .conn
        .prepare("SELECT content FROM mod_bindings ORDER BY id")?;
    let bindings: Vec<Value> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str(&r?)?))
        .collect::<Result<_>>()?;
    Ok(
        json!({"revision":revision(&store)?,"installed":versions.iter().map(Bundle::public).collect::<Vec<_>>(),
        "bindings":bindings,"available_bundled":[bundled()?.public()],
        "unavailable":[{"id":"less-tool-noise","reason":"Planned; external transformers are not implemented"}],
        "support":{"delivery":"unsupported","native_configuration":"unverified","children":"unknown","global_text_suppression":"unsupported"}}),
    )
}
pub fn preview(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _files = FILE_OPERATIONS.lock().unwrap();
    let operation = text(p, "operation")?;
    if !matches!(operation, "install" | "update") {
        return Err(error("invalid_mod", "operation must be install or update"));
    }
    let bundle = import(text(p, "source")?)?;
    let id = format!("mp-{}", uuid::Uuid::new_v4().simple());
    let root = paths::mods_dir().join("previews").join(&id);
    if let Err(e) = write_tree(&root, &bundle) {
        let _ = fs::remove_dir_all(&root);
        return Err(e);
    }
    let result = {
        let store = d.store.lock().unwrap();
        let previous: Vec<Value> = versions(&store)?
            .iter()
            .filter(|v| v.manifest.id == bundle.manifest.id)
            .map(Bundle::public)
            .collect();
        store.conn.execute(
            "INSERT INTO mod_previews(id,content) VALUES(?1,?2)",
            params![id, serde_json::to_string(&bundle)?],
        )?;
        json!({"id":id,"version":bundle.public(),"fingerprint":bundle.fingerprint,"files":bundle.public()["files"],
            "contents":bundle.files,"previous":previous,"operation":operation,"permissions":[],"unsupported":[],
            "notice":"Install copies this preview and never enables the mod"})
    };
    Ok(result)
}
pub fn install(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _files = FILE_OPERATIONS.lock().unwrap();
    if p["confirm"] != true {
        return Err(error(
            "confirmation_required",
            "confirm:true is required to install a mod",
        ));
    }
    let id = text(p, "preview_id")?;
    let (content, result): (String, Option<String>) = d
        .store
        .lock()
        .unwrap()
        .conn
        .query_row(
            "SELECT content,result FROM mod_previews WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| error("invalid_mod", "unknown preview"))?;
    if let Some(result) = result {
        let result: Value = serde_json::from_str(&result)?;
        let still_installed = d
            .store
            .lock()
            .unwrap()
            .conn
            .prepare("SELECT 1 FROM mod_versions WHERE fingerprint=?1")?
            .exists([result["version"]["fingerprint"].as_str().unwrap_or("")])?;
        if !still_installed {
            return Err(error(
                "mod_changed",
                "this preview's version was removed; create a new preview",
            ));
        }
        discard(&paths::mods_dir().join("previews").join(id))?;
        return Ok(result);
    }
    let mut bundle: Bundle = serde_json::from_str(&content)?;
    let target = paths::mods_dir()
        .join("versions")
        .join(&bundle.manifest.id)
        .join(&bundle.fingerprint);
    // DB-held preview bytes are the immutable source of truth, even after restart.
    if !target.exists() {
        let tmp = paths::mods_dir()
            .join("previews")
            .join(format!("install-{}", uuid::Uuid::new_v4().simple()));
        write_tree(&tmp, &bundle)?;
        paths::ensure_private_dir(target.parent().unwrap())?;
        if let Err(e) = fs::rename(&tmp, &target) {
            let _ = fs::remove_dir_all(&tmp);
            if !target.exists() {
                return Err(e.into());
            }
        }
    }
    let (result, event) = {
        let store = d.store.lock().unwrap();
        let tx = store.conn.unchecked_transaction()?;
        // Concurrent retries commit one version and one event.
        let prior: Option<String> =
            tx.query_row("SELECT result FROM mod_previews WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
        if let Some(prior) = prior {
            return Ok(serde_json::from_str(&prior)?);
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT content FROM mod_versions WHERE fingerprint=?1",
                [&bundle.fingerprint],
                |r| r.get(0),
            )
            .optional()?;
        let event;
        if let Some(existing) = existing {
            bundle = serde_json::from_str(&existing)?;
            event = None;
        } else {
            bundle.installed_ms = now();
            tx.execute(
                "INSERT INTO mod_versions(fingerprint,mod_id,content) VALUES(?1,?2,?3)",
                params![
                    bundle.fingerprint,
                    bundle.manifest.id,
                    serde_json::to_string(&bundle)?
                ],
            )?;
            let revision = advance(&store)?;
            event = Some(store.insert_event(now(),None,None,"mods_changed","daemon","exact",&json!({"operation":"install","mod_id":bundle.manifest.id,"fingerprint":bundle.fingerprint,"revision":revision}))?);
        }
        let result = json!({"version":bundle.public(),"revision":revision(&store)?});
        tx.execute(
            "UPDATE mod_previews SET result=?2 WHERE id=?1",
            params![id, result.to_string()],
        )?;
        tx.commit()?;
        (result, event)
    };
    if let Some(event) = event {
        let _ = d.events.send(event);
    }
    let _ = fs::remove_dir_all(paths::mods_dir().join("previews").join(id));
    Ok(result)
}
pub fn remove(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _files = FILE_OPERATIONS.lock().unwrap();
    if p["confirm"] != true {
        return Err(error(
            "confirmation_required",
            "confirm:true is required to remove a mod",
        ));
    }
    let id = text(p, "mod_id")?;
    let fp = text(p, "fingerprint")?;
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || fp.len() != 64
        || !fp
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(error("invalid_mod", "invalid mod id or fingerprint"));
    }
    let (result, event) = {
        let store = d.store.lock().unwrap();
        let present = versions(&store)?
            .into_iter()
            .any(|v| v.manifest.id == id && v.fingerprint == fp);
        if !present {
            let result = json!({"revision":revision(&store)?,"ended_bindings":[],"removed":false});
            drop(store);
            discard(&paths::mods_dir().join("versions").join(id).join(fp))?;
            return Ok(result);
        }
        expect_revision(&store, p)?;
        let tx = store.conn.unchecked_transaction()?;
        let mut stmt = tx.prepare("SELECT id,content FROM mod_bindings")?;
        let bindings: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        drop(stmt);
        let mut ended = Vec::new();
        for (binding_id, content) in bindings {
            let b: Value = serde_json::from_str(&content)?;
            if b["fingerprint"] == fp {
                tx.execute("DELETE FROM mod_bindings WHERE id=?1", [&binding_id])?;
                ended.push(binding_id);
            }
        }
        tx.execute(
            "DELETE FROM mod_versions WHERE fingerprint=?1 AND mod_id=?2",
            params![fp, id],
        )?;
        let revision = advance(&store)?;
        let event = store.insert_event(now(),None,None,"mods_changed","daemon","exact",&json!({"operation":"remove","mod_id":id,"fingerprint":fp,"revision":revision,"ended_bindings":ended}))?;
        tx.commit()?;
        (
            json!({"revision":revision,"ended_bindings":ended,"removed":true}),
            event,
        )
    };
    let _ = d.events.send(event);
    // Identifiers come from a matched installed version, never unchecked path input.
    let target = paths::mods_dir().join("versions").join(id).join(fp);
    discard(&target)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn migration_reopens_an_existing_database_without_losing_state() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.sqlite");
        {
            let store = crate::store::Store::open(&path).unwrap();
            store
                .conn
                .execute("INSERT INTO meta(key,value) VALUES('fixture','keep')", [])
                .unwrap();
            store.conn.execute_batch("INSERT INTO workspaces(id,path,repo_root,common_dir,kind,created_ms) VALUES('w','/fixture','/fixture','/fixture/.git','current',1); INSERT INTO tasks(id,title,prompt,repo_root,workspace_id,created_ms) VALUES('t','Keep task','Keep prompt','/fixture','w',1); INSERT INTO profiles(id,name,harness,home,is_system,created_ms) VALUES('p','Keep account','generic',NULL,0,1); INSERT INTO account_shown(profile_id,email,plan,observed_ms) VALUES('p','fixture@example.invalid','fixture',1);").unwrap();
            store.conn.execute_batch("DROP TABLE turn_mods; DROP TABLE mod_previews; DROP TABLE mod_bindings; DROP TABLE mod_versions; DELETE FROM meta WHERE key='mods_revision';").unwrap();
        }
        for _ in 0..2 {
            let store = crate::store::Store::open(&path).unwrap();
            let value: String = store
                .conn
                .query_row("SELECT value FROM meta WHERE key='fixture'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(value, "keep");
            assert_eq!(super::revision(&store).unwrap(), 0);
            let title: String = store
                .conn
                .query_row("SELECT title FROM tasks WHERE id='t'", [], |r| r.get(0))
                .unwrap();
            let email: String = store
                .conn
                .query_row(
                    "SELECT email FROM account_shown WHERE profile_id='p'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(title, "Keep task");
            assert_eq!(email, "fixture@example.invalid");
            assert!(super::versions(&store).unwrap().is_empty());
        }
    }
    #[test]
    fn bundled_rules_match_the_reviewed_source() {
        assert_eq!(
            include_str!("../../../mods/clear-prose/style.md"),
            include_str!("../../../docs/design/mods/clear-prose-rules.md")
        );
    }
}
