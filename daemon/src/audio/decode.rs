//! Native decoding is isolated from the daemon in a supervised, descriptor-only worker.
//! Budgets are provisional until synthetic runtime qualification; no native player runs here.
use super::pack::MEDIA_LIMIT;
use anyhow::{anyhow, bail, Result};
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static VALIDATION: Mutex<()> = Mutex::new(());

pub(super) struct ReapedChild(pub(super) Child);
impl Drop for ReapedChild {
    fn drop(&mut self) {
        // A terminated child's PID is not reused until it is reaped. Never signal a
        // completed/reaped child's process group; the supervisor marks that state.
        if self.0.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
pub(super) fn group(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
pub(super) fn validate(file: &File, outer_deadline: Instant) -> Result<u64> {
    let _validation = VALIDATION
        .try_lock()
        .map_err(|_| anyhow!("Audio validation is busy; try again after it finishes."))?;
    if !cfg!(target_os = "macos") {
        bail!("WAV and MP3 validation is unavailable on this platform.");
    }
    let deadline = outer_deadline.min(Instant::now() + Duration::from_secs(2));
    if Instant::now() >= deadline {
        bail!("Audio validation exceeded its time limit.");
    }
    let mut command = Command::new(
        std::env::current_exe().map_err(|_| anyhow!("Cannot start audio validation."))?,
    );
    command
        .arg("audio-validate")
        .env_clear()
        .stdin(Stdio::from(file.try_clone().map_err(|_| {
            anyhow!("Cannot hand off the audio descriptor.")
        })?))
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Explicit synthetic observation hook only; no owner environment/profile is
    // inherited by the otherwise env-cleared descriptor-only worker.
    if let Some(gate) = std::env::var_os("OVERSEER_TEST_AUDIO_WORKER_HOLD") {
        if std::path::Path::new(&gate).join("armed").is_file() {
            command.env("OVERSEER_TEST_AUDIO_WORKER_HOLD", gate);
        }
    }
    group(&mut command);
    let mut child = ReapedChild(
        command
            .spawn()
            .map_err(|_| anyhow!("Cannot start audio validation."))?,
    );
    let mut output = child
        .0
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Cannot read validation status."))?;
    let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(output.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        bail!("Cannot supervise audio validation.");
    }
    let mut bytes = Vec::new();
    let mut status = None;
    loop {
        let mut buffer = [0u8; 512];
        match output.read(&mut buffer) {
            Ok(0) if status.is_some() => break,
            Ok(n) => {
                bytes.extend_from_slice(&buffer[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => bail!("Cannot read validation status."),
        }
        if bytes.len() > 4096 {
            bail!("Audio validation returned too much data.");
        }
        if status.is_none() {
            status = child
                .0
                .try_wait()
                .map_err(|_| anyhow!("Cannot supervise audio validation."))?;
        }
        if Instant::now() >= deadline {
            bail!("Audio validation exceeded its time limit.");
        }
        if status.is_none() {
            test_exit_before_memory(&child.0)?;
        }
        if status.is_none() && resident_bytes(child.0.id())? > 96 * 1024 * 1024 {
            bail!("Audio validation exceeded its memory budget.");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !status.is_some_and(|s| s.success()) {
        bail!("Audio is malformed or unsupported.");
    }
    let duration = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|ms| (1..=15_000).contains(ms))
        .ok_or_else(|| anyhow!("Audio is malformed or exceeds 15 seconds."))?;
    Ok(duration)
}

// Test-only observation: release this owned synthetic worker after try_wait
// reported None, then observe its exit without reaping before the unchanged RSS
// check. No RPC, media substitution, timeout extension or error override.
#[cfg(target_os = "macos")]
fn test_exit_before_memory(child: &Child) -> Result<()> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let Some(gate) = std::env::var_os("OVERSEER_TEST_AUDIO_EXIT_BEFORE_MEMORY") else {
        return Ok(());
    };
    if std::env::var_os("OVERSEER_TEST_AUDIO_WORKER_HOLD").as_ref() != Some(&gate) {
        bail!("Synthetic decoder exit gate needs its matching worker gate.");
    }
    let gate = std::path::PathBuf::from(gate);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(gate.join("parent-claimed"))
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(_) => bail!("Cannot claim the synthetic decoder exit gate."),
    }
    let fd = unsafe { libc::kqueue() };
    if fd < 0 {
        bail!("Cannot observe the owned decoder exit.");
    }
    let queue = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut change: libc::kevent = unsafe { std::mem::zeroed() };
    change.ident = child.id() as usize;
    change.filter = libc::EVFILT_PROC;
    change.flags = libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT;
    change.fflags = libc::NOTE_EXIT;
    if unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &change,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } != 0
    {
        bail!("Cannot register the owned decoder exit.");
    }
    std::fs::write(gate.join("release"), b"release")
        .map_err(|_| anyhow!("Cannot release the synthetic decoder."))?;
    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
    let timeout = libc::timespec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    let received = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            std::ptr::null(),
            0,
            &mut event,
            1,
            &timeout,
        )
    };
    if received != 1
        || event.ident != child.id() as usize
        || event.filter != libc::EVFILT_PROC
        || event.fflags & libc::NOTE_EXIT == 0
        || event.flags & libc::EV_ERROR != 0
    {
        bail!("Owned decoder exit was not observed within the synthetic bound.");
    }
    // NOTE_EXIT can precede publication of waitable status. WNOWAIT preserves
    // the owned zombie; readiness checks are bounded, not an arbitrary sleep.
    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
            )
        } != 0
        {
            bail!("Cannot inspect the owned decoder without reaping.");
        }
        if info.si_signo == libc::SIGCHLD {
            if info.si_pid != child.id() as i32 {
                bail!("Synthetic exit did not match the owned decoder.");
            }
            std::fs::write(gate.join("exit-observed.json"), serde_json::json!({
                "pid":child.id(),"wait_code":info.si_code,"exit_status":info.si_status,"observed_without_reap":true,
                "kill_zero":unsafe {libc::kill(child.id() as i32,0)}}).to_string())
                .map_err(|_|anyhow!("Cannot record the synthetic decoder exit."))?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("Owned decoder did not become waitable.");
        }
        std::thread::yield_now();
    }
}
#[cfg(not(target_os = "macos"))]
fn test_exit_before_memory(_child: &Child) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn resident_bytes(pid: u32) -> Result<u64> {
    // libproc.h/sys/proc_info.h PROC_PIDTASKINFO=4, SDK field order retained.
    #[repr(C)]
    #[derive(Default)]
    struct TaskInfo {
        wide: [u64; 6],
        narrow: [i32; 12],
    }
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut libc::c_void,
            size: i32,
        ) -> i32;
    }
    let mut info = TaskInfo::default();
    let size = std::mem::size_of::<TaskInfo>() as i32;
    let read = unsafe { proc_pidinfo(pid as i32, 4, 0, (&mut info as *mut TaskInfo).cast(), size) };
    if read != size {
        // Exit may race the inspection. A live worker with unreadable footprint
        // fails closed; no unsupported RSS hard-limit claim is made.
        if unsafe { libc::kill(pid as i32, 0) } == 0 {
            bail!("Cannot supervise audio validation memory.");
        }
        return Ok(0);
    }
    Ok(info.wide[1])
}
#[cfg(not(target_os = "macos"))]
fn resident_bytes(_pid: u32) -> Result<u64> {
    bail!("Audio validation is unavailable.")
}

/// Hidden command, dispatched before daemon/profile initialization. Only FD 0 is input;
/// stdout is one bounded duration integer and errors never include file bytes or paths.
pub(crate) fn worker() -> i32 {
    if super::test_hold("WORKER", &serde_json::json!({"pid":std::process::id()})).is_err() {
        return 1;
    }
    #[cfg(target_os = "macos")]
    {
        match native::decode() {
            Ok(ms) => {
                println!("{ms}");
                0
            }
            Err(_) => 1,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        1
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::ffi::c_void;
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::FileExt;
    type Ref = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Format {
        rate: f64,
        id: u32,
        flags: u32,
        packet_bytes: u32,
        packet_frames: u32,
        frame_bytes: u32,
        channels: u32,
        bits: u32,
        reserved: u32,
    }
    #[repr(C)]
    struct Buffer {
        channels: u32,
        bytes: u32,
        data: *mut c_void,
    }
    #[repr(C)]
    struct Buffers {
        count: u32,
        buffer: Buffer,
    }
    type ReadFn = extern "C" fn(*mut c_void, i64, u32, *mut c_void, *mut u32) -> i32;
    type SizeFn = extern "C" fn(*mut c_void) -> i64;
    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioFileOpenWithCallbacks(
            client: Ref,
            read: ReadFn,
            write: Option<ReadFn>,
            size: SizeFn,
            set_size: Option<extern "C" fn(Ref, i64) -> i32>,
            hint: u32,
            file: *mut Ref,
        ) -> i32;
        fn AudioFileClose(file: Ref) -> i32;
        fn AudioFileGetProperty(file: Ref, property: u32, size: *mut u32, data: Ref) -> i32;
        fn ExtAudioFileWrapAudioFileID(file: Ref, writing: u8, ext: *mut Ref) -> i32;
        fn ExtAudioFileDispose(file: Ref) -> i32;
        fn ExtAudioFileGetProperty(file: Ref, property: u32, size: *mut u32, data: Ref) -> i32;
        fn ExtAudioFileSetProperty(file: Ref, property: u32, size: u32, data: *const c_void)
            -> i32;
        fn ExtAudioFileRead(file: Ref, frames: *mut u32, data: *mut Buffers) -> i32;
    }
    struct Handles {
        audio: Ref,
        ext: Ref,
    }
    impl Drop for Handles {
        fn drop(&mut self) {
            unsafe {
                if !self.ext.is_null() {
                    ExtAudioFileDispose(self.ext);
                }
                if !self.audio.is_null() {
                    AudioFileClose(self.audio);
                }
            }
        }
    }
    struct Input {
        file: File,
        len: u64,
    }
    extern "C" fn read(c: Ref, pos: i64, count: u32, data: Ref, actual: *mut u32) -> i32 {
        if c.is_null() || actual.is_null() || data.is_null() || pos < 0 {
            return -1;
        }
        let input = unsafe { &*(c.cast::<Input>()) };
        unsafe {
            *actual = 0;
        }
        if pos as u64 >= input.len {
            return 0;
        }
        let n = (count as u64).min(input.len - pos as u64) as usize;
        // AudioToolbox owns this buffer of requestCount bytes; only the bounded
        // remainder is exposed to Rust. No allocation/panic crosses the callback.
        let buffer = unsafe { std::slice::from_raw_parts_mut(data.cast::<u8>(), n) };
        match input.file.read_at(buffer, pos as u64) {
            Ok(n) => {
                unsafe {
                    *actual = n as u32;
                }
                0
            }
            Err(_) => -1,
        }
    }
    extern "C" fn size(c: Ref) -> i64 {
        if c.is_null() {
            return 0;
        }
        unsafe { (*(c.cast::<Input>())).len as i64 }
    }
    fn code(bytes: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*bytes)
    }
    pub(super) fn decode() -> Result<u64> {
        let file = unsafe { File::from_raw_fd(0) };
        let m = file.metadata()?;
        if !m.is_file() || !(1..=MEDIA_LIMIT).contains(&m.len()) {
            bail!("invalid input");
        }
        let mut input = Box::new(Input { file, len: m.len() });
        let mut h = Handles {
            audio: std::ptr::null_mut(),
            ext: std::ptr::null_mut(),
        };
        if unsafe {
            AudioFileOpenWithCallbacks(
                (&mut *input as *mut Input).cast(),
                read,
                None,
                size,
                None,
                0,
                &mut h.audio,
            )
        } != 0
        {
            bail!("invalid audio");
        }
        let mut kind = 0u32;
        let mut n = 4;
        if unsafe {
            AudioFileGetProperty(
                h.audio,
                code(b"ffmt"),
                &mut n,
                (&mut kind as *mut u32).cast(),
            )
        } != 0
            || n != 4
            || ![code(b"WAVE"), code(b"MPG3")].contains(&kind)
        {
            bail!("unsupported audio");
        }
        if unsafe { ExtAudioFileWrapAudioFileID(h.audio, 0, &mut h.ext) } != 0 {
            bail!("invalid audio");
        }
        let mut format = Format::default();
        let mut n = std::mem::size_of::<Format>() as u32;
        if unsafe {
            ExtAudioFileGetProperty(
                h.ext,
                code(b"ffmt"),
                &mut n,
                (&mut format as *mut Format).cast(),
            )
        } != 0
            || n != std::mem::size_of::<Format>() as u32
            || !format.rate.is_finite()
            || !(8000.0..=192000.0).contains(&format.rate)
            || !(1..=2).contains(&format.channels)
        {
            bail!("unsupported format");
        }
        let mut expected = 0i64;
        let mut n = 8;
        if unsafe {
            ExtAudioFileGetProperty(
                h.ext,
                code(b"#frm"),
                &mut n,
                (&mut expected as *mut i64).cast(),
            )
        } != 0
            || n != 8
            || expected <= 0
            || expected as f64 > format.rate * 15.0
        {
            bail!("invalid duration");
        }
        let client = Format {
            rate: format.rate,
            id: code(b"lpcm"),
            flags: 1 | 8,
            packet_bytes: 4 * format.channels,
            packet_frames: 1,
            frame_bytes: 4 * format.channels,
            channels: format.channels,
            bits: 32,
            reserved: 0,
        };
        if unsafe {
            ExtAudioFileSetProperty(
                h.ext,
                code(b"cfmt"),
                std::mem::size_of::<Format>() as u32,
                (&client as *const Format).cast(),
            )
        } != 0
        {
            bail!("unsupported decode");
        }
        let mut pcm = [0f32; 8192];
        let mut total = 0u64;
        loop {
            let mut frames = 4096u32;
            let mut buffers = Buffers {
                count: 1,
                buffer: Buffer {
                    channels: format.channels,
                    bytes: 4096 * format.channels * 4,
                    data: pcm.as_mut_ptr().cast(),
                },
            };
            if unsafe { ExtAudioFileRead(h.ext, &mut frames, &mut buffers) } != 0 || frames > 4096 {
                bail!("malformed decode");
            }
            if frames == 0 {
                break;
            }
            total += frames as u64;
            if total as f64 > format.rate * 15.0
                || pcm[..frames as usize * format.channels as usize]
                    .iter()
                    .any(|v| !v.is_finite())
            {
                bail!("invalid decoded audio");
            }
        }
        // Conservative qualification: inconsistent/truncated native frame counts refuse.
        if total != expected as u64 {
            bail!("inconsistent audio length");
        }
        Ok(((total as f64 * 1000.0 / format.rate).ceil() as u64).max(1))
    }
}
