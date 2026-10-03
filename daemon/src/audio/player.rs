//! One transient player. Private media stays on its owned descriptor from validation to playback.
use super::{
    decode::{group, ReapedChild},
    lines::Line,
    source,
};
use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use std::io::{Seek, SeekFrom, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// Deliberately false until an allocated isolated macOS descriptor/seek/playback
// check proves afplay /dev/fd/0 works. No pathname or copied-media fallback.
const NATIVE_DESCRIPTOR_PLAYER_QUALIFIED: bool = false;
static SERIAL: Mutex<()> = Mutex::new(());
static ACTIVE: Mutex<Option<u32>> = Mutex::new(None);
static CANCEL_EPOCH: AtomicU64 = AtomicU64::new(0);

pub(super) fn available() -> bool {
    std::env::var_os("OVERSEER_TEST_AUDIO_UNAVAILABLE").is_none()
        && (std::env::var_os("OVERSEER_TEST_AUDIO_LOG").is_some()
            || (NATIVE_DESCRIPTOR_PLAYER_QUALIFIED
                && cfg!(target_os = "macos")
                && std::path::Path::new("/usr/bin/afplay").is_file()))
}
/// Cancelling does not need the serial playback guard. The owned child is reaped
/// by its supervisor; ACTIVE is cleared under the same lock as try_wait to avoid
/// signalling a PID that the supervisor has already reaped.
pub(super) fn cancel(_d: &Arc<Daemon>) {
    let active = ACTIVE.lock().unwrap();
    CANCEL_EPOCH.fetch_add(1, Ordering::SeqCst);
    if let Some(pid) = *active {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}
pub(super) fn play(d: &Arc<Daemon>, line: Line, preview: bool) -> Result<()> {
    play_checked(d, line, preview, || true).map(|_| ())
}
/// True only when actually admitted/completed. The semantic guard runs after
/// decoding, before each actual receipt/spawn; false never consumes a live ticket.
pub(super) fn play_checked(
    d: &Arc<Daemon>,
    line: Line,
    preview: bool,
    still_current: impl Fn() -> bool,
) -> Result<bool> {
    let _serial = SERIAL
        .try_lock()
        .map_err(|_| anyhow!("Audio is already playing."))?;
    let epoch = CANCEL_EPOCH.load(Ordering::SeqCst);
    for attempt in 0..2 {
        if CANCEL_EPOCH.load(Ordering::SeqCst) != epoch || !still_current() {
            return Ok(false);
        }
        // Resolve now, after the semantic worker's arbiter wait, never from queued settings.
        let source = source::snapshot(d)?;
        if !preview && !source.enabled {
            return Ok(false);
        }
        let mut opened = source.open_line(line)?;
        super::test_hold(
            "DECODED",
            &serde_json::json!({"revision":source.revision,"key":line.key()}),
        )?;
        if CANCEL_EPOCH.load(Ordering::SeqCst) != epoch {
            return Ok(false);
        }
        // Seek/read validation belongs outside the short settings/admission boundary.
        opened
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|_| anyhow!("Cannot rewind the opened audio file."))?;
        let admission = source::admission_guard();
        if CANCEL_EPOCH.load(Ordering::SeqCst) != epoch || !still_current() {
            return Ok(false);
        }
        if source::current_revision(d)? != source.revision {
            // The clip has not started. Discard its owned FD and resolve the same
            // still-current semantic cue against the newly selected pack, outside locks.
            drop(admission);
            drop(opened);
            if attempt == 1 {
                bail!("Audio source kept changing; this cue was skipped before playback.");
            }
            continue;
        }
        if let Some(path) = std::env::var_os("OVERSEER_TEST_AUDIO_LOG") {
            // The sink follows the same safe file open/full decoder boundary. Its receipt
            // proves routing only, not audible speech or afplay descriptor qualification.
            let _active = ACTIVE.lock().unwrap();
            if CANCEL_EPOCH.load(Ordering::SeqCst) != epoch {
                return Ok(false);
            }
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|_| anyhow!("Cannot record the synthetic audio receipt."))?;
            writeln!(log, "{}:{}", opened.pack_id, line.key())?;
            return Ok(true);
        }
        if !NATIVE_DESCRIPTOR_PLAYER_QUALIFIED {
            bail!("Opened-descriptor playback is awaiting qualification.");
        }
        let mut command = Command::new("/usr/bin/afplay");
        command
            .arg("/dev/fd/0")
            .env_clear()
            .stdin(Stdio::from(opened.file))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        group(&mut command);
        let mut child = {
            let mut active = ACTIVE.lock().unwrap();
            if CANCEL_EPOCH.load(Ordering::SeqCst) != epoch {
                return Ok(false);
            }
            let child = ReapedChild(
                command
                    .spawn()
                    .map_err(|_| anyhow!("Cannot start the audio player."))?,
            );
            *active = Some(child.0.id());
            child
        };
        drop(admission);
        let deadline = Instant::now() + Duration::from_millis(opened.duration_ms + 2000);
        loop {
            {
                let mut active = ACTIVE.lock().unwrap();
                match child.0.try_wait() {
                    Ok(Some(status)) => {
                        *active = None;
                        return if CANCEL_EPOCH.load(Ordering::SeqCst) != epoch {
                            Ok(false)
                        } else if status.success() {
                            Ok(true)
                        } else {
                            Err(anyhow!("The audio player could not play this file."))
                        };
                    }
                    Err(_) => {
                        *active = None;
                        bail!("Cannot supervise the audio player.");
                    }
                    Ok(None) => {}
                }
                if Instant::now() >= deadline || CANCEL_EPOCH.load(Ordering::SeqCst) != epoch {
                    unsafe {
                        libc::kill(-(child.0.id() as i32), libc::SIGKILL);
                    }
                    // Keep the PID owned until kill, then remove it before the Drop reap.
                    *active = None;
                    if CANCEL_EPOCH.load(Ordering::SeqCst) != epoch {
                        return Ok(false);
                    }
                    bail!("The audio player exceeded its time limit.");
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Err(anyhow!(
        "Audio source changed repeatedly; this cue was skipped."
    ))
}
