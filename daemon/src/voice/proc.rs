//! The listener process (AC-163): started by the daemon, spoken to over its standard input and
//! heard on its standard output, one JSON object a line.
//!
//! On macOS the daemon is started by VS Code, so without care macOS would ask for the microphone in
//! VS Code's name. The listener is spawned with its responsibility disclaimed, so macOS treats it as
//! its own app (`Overseer Listener.app`) and asks in Overseer's name. When that is not possible the
//! listener starts plainly and the prompt names whoever started the daemon.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::ffi::CString;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::path::Path;
use std::sync::Mutex;

pub struct Listener {
    pub pid: i32,
    stdin: Mutex<std::fs::File>,
}

extern "C" {
    static environ: *const *const libc::c_char;
}

impl Listener {
    /// Starts the listener; returns it and its standard output.
    pub fn spawn(path: &Path, args: &[String], env: &[(String, String)]) -> Result<(Listener, std::fs::File)> {
        let prog = CString::new(path.as_os_str().as_encoded_bytes())?;
        let mut argv: Vec<CString> = vec![prog.clone()];
        for a in args {
            argv.push(CString::new(a.as_str())?);
        }
        let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|c| c.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        // The daemon's environment, with the extras on top.
        let mut envs: Vec<CString> = Vec::new();
        unsafe {
            let mut p = environ;
            while !(*p).is_null() {
                let entry = std::ffi::CStr::from_ptr(*p).to_bytes();
                let key = entry.split(|b| *b == b'=').next().unwrap_or_default();
                if !env.iter().any(|(k, _)| k.as_bytes() == key) {
                    envs.push(CString::from(std::ffi::CStr::from_ptr(*p)));
                }
                p = p.add(1);
            }
        }
        for (k, v) in env {
            envs.push(CString::new(format!("{k}={v}"))?);
        }
        let mut env_ptrs: Vec<*const libc::c_char> = envs.iter().map(|c| c.as_ptr()).collect();
        env_ptrs.push(std::ptr::null());

        unsafe {
            let mut child_in = [0i32; 2];
            let mut child_out = [0i32; 2];
            if libc::pipe(child_in.as_mut_ptr()) != 0 || libc::pipe(child_out.as_mut_ptr()) != 0 {
                bail!("could not make pipes for the listener");
            }
            let mut actions: libc::posix_spawn_file_actions_t = std::mem::zeroed();
            libc::posix_spawn_file_actions_init(&mut actions);
            libc::posix_spawn_file_actions_adddup2(&mut actions, child_in[0], 0);
            libc::posix_spawn_file_actions_adddup2(&mut actions, child_out[1], 1);
            let devnull = CString::new("/dev/null").unwrap();
            libc::posix_spawn_file_actions_addopen(&mut actions, 2, devnull.as_ptr(), libc::O_WRONLY, 0);
            let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
            libc::posix_spawnattr_init(&mut attr);
            #[cfg(target_os = "macos")]
            {
                // Only the descriptors set up above reach the listener.
                const POSIX_SPAWN_CLOEXEC_DEFAULT: libc::c_short = 0x4000;
                libc::posix_spawnattr_setflags(&mut attr, POSIX_SPAWN_CLOEXEC_DEFAULT);
                let name = CString::new("responsibility_spawnattrs_setdisclaim").unwrap();
                let f = libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr());
                if !f.is_null() {
                    let disclaim: extern "C" fn(*mut libc::posix_spawnattr_t, libc::c_int) -> libc::c_int = std::mem::transmute(f);
                    disclaim(&mut attr, 1);
                }
            }
            let mut pid: libc::pid_t = 0;
            let rc = libc::posix_spawn(&mut pid, prog.as_ptr(), &actions, &attr, argv_ptrs.as_ptr() as *const *mut libc::c_char, env_ptrs.as_ptr() as *const *mut libc::c_char);
            libc::posix_spawn_file_actions_destroy(&mut actions);
            libc::posix_spawnattr_destroy(&mut attr);
            libc::close(child_in[0]);
            libc::close(child_out[1]);
            if rc != 0 {
                libc::close(child_in[1]);
                libc::close(child_out[0]);
                return Err(anyhow!("could not start the listener: {}", std::io::Error::from_raw_os_error(rc)));
            }
            libc::fcntl(child_in[1], libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(child_out[0], libc::F_SETFD, libc::FD_CLOEXEC);
            let stdin = std::fs::File::from_raw_fd(child_in[1]);
            let stdout = std::fs::File::from_raw_fd(child_out[0]);
            Ok((Listener { pid, stdin: Mutex::new(stdin) }, stdout))
        }
    }

    pub fn send(&self, cmd: &Value) -> Result<()> {
        let mut w = self.stdin.lock().unwrap();
        writeln!(w, "{cmd}")?;
        w.flush()?;
        Ok(())
    }

    /// Asks the listener to quit, and makes sure it has within half a second.
    pub fn stop(&self) {
        let _ = self.send(&serde_json::json!({"cmd": "quit"}));
        for _ in 0..25 {
            if !alive(self.pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
        }
    }
}

/// Whether the process still runs (it is reaped by the reader thread when it ends).
pub fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Waits for the listener to end and says how.
pub fn reap(pid: i32) -> String {
    let mut status = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }
    if libc::WIFEXITED(status) {
        format!("exited with code {}", libc::WEXITSTATUS(status))
    } else if libc::WIFSIGNALED(status) {
        format!("killed by signal {}", libc::WTERMSIG(status))
    } else {
        "ended".into()
    }
}
