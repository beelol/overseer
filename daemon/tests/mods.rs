mod common;
use common::*;
use serde_json::{json, Value};

fn install(d: &Daemon) -> Value {
    let preview = d.call(
        "mods.preview",
        json!({"source":"bundled:clear-prose", "operation":"install"}),
    );
    d.call(
        "mods.install",
        json!({"preview_id":preview["id"], "confirm":true}),
    )
}

#[test]
fn install_is_not_enable_and_requires_confirmation() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let list = d.call("mods.list", json!({}));
    assert!(list["installed"].as_array().unwrap().is_empty());
    assert_eq!(list["available_bundled"][0]["id"], "clear-prose");
    let preview = d.call(
        "mods.preview",
        json!({"source":"bundled:clear-prose", "operation":"install"}),
    );
    assert!(d
        .try_call("mods.install", json!({"preview_id":preview["id"]}))
        .unwrap_err()
        .contains("confirm"));
    let first = d.call(
        "mods.install",
        json!({"preview_id":preview["id"], "confirm":true}),
    );
    let again = d.call(
        "mods.install",
        json!({"preview_id":preview["id"], "confirm":true}),
    );
    assert_eq!(first, again);
    let list = d.call("mods.list", json!({}));
    assert_eq!(list["installed"].as_array().unwrap().len(), 1);
    assert!(list["bindings"].as_array().unwrap().is_empty());
    assert_eq!(first["version"]["fingerprint"].as_str().unwrap().len(), 64);
    assert!(first["version"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"] == "style.md"));
}

#[test]
fn immutable_preview_survives_source_change_and_restart() {
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let folder = t.path().join("mod");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("mod.toml"), "schema_version=1\nid='fixture'\nname='Fixture'\nversion='1'\nsummary='A rule'\nsource='local'\n[rules]\nfiles=['rule.md']\n").unwrap();
    std::fs::write(folder.join("rule.md"), "Keep all warnings.").unwrap();
    let preview = d.call(
        "mods.preview",
        json!({"source":folder, "operation":"install"}),
    );
    std::fs::write(folder.join("rule.md"), "DROP warnings.").unwrap();
    let installed = d.call(
        "mods.install",
        json!({"preview_id":preview["id"],"confirm":true}),
    );
    let fp = installed["version"]["fingerprint"].as_str().unwrap();
    let root = d.home.path().join("mods/versions/fixture").join(fp);
    assert_eq!(
        std::fs::read_to_string(root.join("rule.md")).unwrap(),
        "Keep all warnings."
    );
    let mut child = d.child.take().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    d.spawn();
    assert_eq!(
        d.call("mods.list", json!({}))["installed"][0]["fingerprint"],
        fp
    );
}

#[test]
fn import_refuses_path_escapes_and_code() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let folder = t.path().join("mod");
    std::fs::create_dir(&folder).unwrap();
    let manifest =
        "schema_version=1\nid='bad'\nname='Bad'\nversion='1'\nsummary='Bad'\nsource='local'\n";
    std::fs::write(
        folder.join("mod.toml"),
        format!("{manifest}[rules]\nfiles=['../secret.md']\n"),
    )
    .unwrap();
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err());
    std::fs::write(
        folder.join("mod.toml"),
        format!("{manifest}[program]\nbuild='touch outside'\n"),
    )
    .unwrap();
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .unwrap_err()
        .contains("unsupported"));
    std::fs::write(
        folder.join("mod.toml"),
        format!("{manifest}[style]\nfile='style.md'\n"),
    )
    .unwrap();
    std::os::unix::fs::symlink("/etc/passwd", folder.join("style.md")).unwrap();
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err());
    assert!(d.call("mods.list", json!({}))["installed"]
        .as_array()
        .unwrap()
        .is_empty());
}

fn local_mod(folder: &std::path::Path, id: &str, contents: &[u8]) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join("mod.toml"), format!("schema_version=1\nid='{id}'\nname='Fixture'\nversion='1'\nsummary='Rule'\nsource='local'\n[rules]\nfiles=['rule.md']\n")).unwrap();
    std::fs::write(folder.join("rule.md"), contents).unwrap();
}

#[test]
fn import_boundaries_schema_and_forbidden_launch_fields() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let folder = t.path().join("bounded");
    local_mod(&folder, "bounded", &vec![b'x'; 32 * 1024]);
    d.call(
        "mods.preview",
        json!({"source":folder,"operation":"install"}),
    );
    std::fs::write(folder.join("rule.md"), vec![b'x'; 32 * 1024 + 1]).unwrap();
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err());
    local_mod(&folder, "bounded", b"Keep evidence.");
    let base = std::fs::read_to_string(folder.join("mod.toml")).unwrap();
    let padded = format!("{base}#{}", " ".repeat(64 * 1024 - base.len() - 1));
    std::fs::write(folder.join("mod.toml"), &padded).unwrap();
    d.call(
        "mods.preview",
        json!({"source":folder,"operation":"install"}),
    );
    std::fs::write(folder.join("mod.toml"), format!("{padded} ")).unwrap();
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err());
    for extra in [
        "schema_version=99",
        "extra_args=['--dangerously-bypass-approvals-and-sandbox']",
        "env={HOME='/outside'}",
        "permission_mode='bypassPermissions'",
    ] {
        let manifest = if extra.starts_with("schema_version") {
            base.replace("schema_version=1", extra)
        } else {
            format!("{extra}\n{base}")
        };
        std::fs::write(folder.join("mod.toml"), manifest).unwrap();
        assert!(
            d.try_call(
                "mods.preview",
                json!({"source":folder,"operation":"install"})
            )
            .is_err(),
            "accepted {extra}"
        );
    }
    local_mod(&folder, "bounded", &[0xff]);
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err());
    local_mod(&folder, "bounded", b"Keep evidence.");
    for i in 0..126 {
        std::fs::write(folder.join(format!("extra-{i}")), b"").unwrap();
    }
    d.call(
        "mods.preview",
        json!({"source":folder,"operation":"install"}),
    );
    std::fs::write(folder.join("one-too-many"), b"").unwrap();
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err());
}

#[test]
fn preview_versions_are_pinned_and_remove_is_confirmed() {
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let folder = t.path().join("fixture");
    local_mod(&folder, "fixture", b"First version.");
    let preview = d.call(
        "mods.preview",
        json!({"source":folder,"operation":"install"}),
    );
    let first = d.call(
        "mods.install",
        json!({"preview_id":preview["id"],"confirm":true}),
    );
    std::fs::write(folder.join("rule.md"), b"Second version.").unwrap();
    let preview2 = d.call(
        "mods.preview",
        json!({"source":folder,"operation":"update"}),
    );
    let second = d.call(
        "mods.install",
        json!({"preview_id":preview2["id"],"confirm":true}),
    );
    assert_ne!(
        first["version"]["fingerprint"],
        second["version"]["fingerprint"]
    );
    assert_eq!(
        d.call("mods.list", json!({}))["installed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let remove = json!({"mod_id":"fixture","fingerprint":first["version"]["fingerprint"],"expected_revision":second["revision"]});
    assert!(d
        .try_call("mods.remove", remove.clone())
        .unwrap_err()
        .contains("confirm"));
    let mut remove = remove;
    remove["confirm"] = json!(true);
    let removed = d.call("mods.remove", remove.clone());
    assert_eq!(removed["removed"], true);
    assert_eq!(d.call("mods.remove", remove)["removed"], false);
    assert_eq!(
        d.call("mods.list", json!({}))["installed"][0]["fingerprint"],
        second["version"]["fingerprint"]
    );
    let orphan = d.home.path().join("mods/previews/interrupted-stage");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join("partial"), b"unfinished").unwrap();
    let mut child = d.child.take().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    d.spawn();
    assert!(!orphan.exists());
    let out = std::process::Command::new(BIN)
        .args(["ctl", "mods.list", "{}"])
        .env("OVERSEER_HOME", d.home.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cli: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        cli["result"]["installed"][0]["fingerprint"],
        second["version"]["fingerprint"]
    );
}

fn restart(d: &mut Daemon) {
    let mut child = d.child.take().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    d.spawn();
}

#[test]
fn recovery_reconciles_committed_previews_and_orphan_versions() {
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let preview = d.call(
        "mods.preview",
        json!({"source":"bundled:clear-prose","operation":"install"}),
    );
    let installed = d.call(
        "mods.install",
        json!({"preview_id":preview["id"],"confirm":true}),
    );
    let stage = d
        .home
        .path()
        .join("mods/previews")
        .join(preview["id"].as_str().unwrap());
    std::fs::create_dir_all(&stage).unwrap(); // crash after commit, before preview cleanup
    let root = d
        .home
        .path()
        .join("mods/versions/clear-prose")
        .join(installed["version"]["fingerprint"].as_str().unwrap());
    std::fs::remove_file(root.join("style.md")).unwrap(); // incomplete installed tree
    let orphan = d
        .home
        .path()
        .join("mods/versions/orphan")
        .join("a".repeat(64));
    std::fs::create_dir_all(&orphan).unwrap(); // rename before install commit, or remove before cleanup
    std::fs::write(orphan.join("partial"), b"unfinished").unwrap();
    restart(&mut d);
    assert!(!stage.exists());
    assert!(!orphan.exists());
    assert_eq!(
        std::fs::read_to_string(root.join("style.md")).unwrap(),
        include_str!("../../mods/clear-prose/style.md")
    );
    let remove = json!({"mod_id":"clear-prose","fingerprint":installed["version"]["fingerprint"],"confirm":true,"expected_revision":installed["revision"]});
    d.call("mods.remove", remove.clone());
    std::fs::create_dir_all(&root).unwrap(); // failed cleanup retried after the DB already removed it
    std::fs::write(root.join("partial"), b"unfinished").unwrap();
    d.call("mods.remove", remove);
    assert!(!root.exists());
    assert!(
        d.try_call(
            "mods.install",
            json!({"preview_id":preview["id"],"confirm":true})
        )
        .is_err(),
        "old preview cannot report a removed version as installed"
    );
}

#[test]
fn library_distinguishes_message_delivery_from_runtime_qualification() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    assert_eq!(
        d.call("mods.list", json!({}))["support"]["delivery"],
        "message_text"
    );
    let support=d.call("mods.list",json!({}))["support"].clone();
    assert_eq!(support["native_configuration"],"unverified");
    assert_eq!(support["installed_runtime_qualification"],"unverified");
    assert_eq!(support["children"],"unknown");
    assert_eq!(support["global_text_suppression"],"unsupported");

}

#[test]
fn import_is_bounded_for_empty_trees_total_bytes_and_special_files() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let folder = t.path().join("fixture");
    local_mod(&folder, "fixture", b"Keep warnings.");
    for n in 0..256 {
        std::fs::create_dir(folder.join(format!("empty-{n}"))).unwrap();
    }
    assert!(
        d.try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err(),
        "empty directories must be bounded"
    );
    for n in 0..256 {
        std::fs::remove_dir(folder.join(format!("empty-{n}"))).unwrap();
    }
    let manifest_bytes = std::fs::metadata(folder.join("mod.toml")).unwrap().len() as usize;
    for n in 0..31 {
        std::fs::write(folder.join(format!("bytes-{n}")), vec![b'x'; 32 * 1024]).unwrap();
    }
    std::fs::write(
        folder.join("rule.md"),
        vec![b'x'; 32 * 1024 - manifest_bytes],
    )
    .unwrap();
    d.call(
        "mods.preview",
        json!({"source":folder,"operation":"install"}),
    );
    std::fs::write(folder.join("one-too-many-byte"), b"x").unwrap();
    assert!(d
        .try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err());
    std::fs::remove_file(folder.join("one-too-many-byte")).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let fifo = std::ffi::CString::new(folder.join("fifo").as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(
        d.try_call(
            "mods.preview",
            json!({"source":folder,"operation":"install"})
        )
        .is_err(),
        "FIFO must be rejected without blocking"
    );
}

#[test]
fn failed_remove_cleanup_is_retried_without_resurrecting_the_version() {
    use std::os::unix::fs::PermissionsExt;
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let installed = install(&d);
    let fp = installed["version"]["fingerprint"].as_str().unwrap();
    let parent = d.home.path().join("mods/versions/clear-prose");
    let root = parent.join(fp);
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o500)).unwrap();
    let p = json!({"mod_id":"clear-prose","fingerprint":fp,"confirm":true,"expected_revision":installed["revision"]});
    let failed = d.try_call("mods.remove", p.clone());
    // Restore permissions even if the assertion below fails, so fixture cleanup is reliable.
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        failed.is_err(),
        "fixture must exercise a real filesystem deletion failure"
    );
    assert!(d.call("mods.list", json!({}))["installed"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(root.exists(), "failed cleanup must leave a retry target");
    d.call("mods.remove", p);
    assert!(!root.exists());
}
