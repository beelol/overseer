//! Keeps the Mac awake while it matters (AC-123): while phone access is on and an agent is
//! active or waiting for the owner, the daemon holds a power assertion that prevents idle sleep.
//! A closed lid on battery sleeps anyway; no assertion changes that.

use crate::daemon::{Daemon, ACTIVE};
use serde_json::json;
use std::sync::{Arc, Mutex};

pub const NAME: &str = "Overseer: agents are running and phone access is on";

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{c_char, c_void, CString};

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, text: *const c_char, encoding: u32) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(kind: *const c_void, level: u32, name: *const c_void, id: *mut u32) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    const UTF8: u32 = 0x0800_0100;
    const LEVEL_ON: u32 = 255;

    pub struct Assertion(u32);

    fn cf(text: &str) -> Option<*const c_void> {
        let c = CString::new(text).ok()?;
        let s = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
        (!s.is_null()).then_some(s)
    }

    pub fn hold(name: &str) -> Result<Assertion, String> {
        let kind = cf("PreventUserIdleSystemSleep").ok_or("no string")?;
        let label = cf(name).ok_or("no string")?;
        let mut id = 0u32;
        let rc = unsafe { IOPMAssertionCreateWithName(kind, LEVEL_ON, label, &mut id) };
        unsafe {
            CFRelease(kind);
            CFRelease(label);
        }
        if rc == 0 {
            Ok(Assertion(id))
        } else {
            Err(format!("IOPMAssertionCreateWithName returned {rc}"))
        }
    }

    impl Drop for Assertion {
        fn drop(&mut self) {
            unsafe { IOPMAssertionRelease(self.0) };
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    /// Linux keeps the machine awake with `systemd-inhibit`, which lives as long as its child.
    pub struct Assertion(std::process::Child);

    pub fn hold(name: &str) -> Result<Assertion, String> {
        std::process::Command::new("systemd-inhibit")
            .args(["--what=idle:sleep", "--who=Overseer", &format!("--why={name}"), "--mode=block", "sleep", "infinity"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(Assertion)
            .map_err(|e| e.to_string())
    }

    impl Drop for Assertion {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[derive(Default)]
pub struct Power {
    held: Mutex<Option<platform::Assertion>>,
}

impl Power {
    pub fn is_held(&self) -> bool {
        self.held.lock().unwrap().is_some()
    }
}

/// Takes or releases the assertion to match what is true now. Every change is an event.
pub fn reconcile(d: &Arc<Daemon>) {
    let phone_access = d.gateway.port().is_some();
    let needing: Vec<String> = match d.store.lock().unwrap().runs() {
        Ok(runs) => runs.into_iter().filter(|r| r.parent_run_id.is_none() && ACTIVE.contains(&r.status.as_str())).map(|r| r.id).collect(),
        Err(_) => return,
    };
    let wanted = phone_access && !needing.is_empty();
    let mut held = d.gateway.power.held.lock().unwrap();
    if wanted == held.is_some() {
        return;
    }
    if wanted {
        match platform::hold(NAME) {
            Ok(assertion) => {
                *held = Some(assertion);
                drop(held);
                let _ = d.emit(None, None, "power", "daemon", "exact", json!({"awake": true, "why": "phone access is on and agents are active", "runs": needing}));
            }
            Err(e) => crate::log(&format!("power: the Mac could not be kept awake: {e}")),
        }
    } else {
        *held = None;
        drop(held);
        let why = if phone_access { "no agent is active" } else { "phone access is off" };
        let _ = d.emit(None, None, "power", "daemon", "exact", json!({"awake": false, "why": why}));
    }
}

/// Follows the daemon's events: a status change or a change of phone access is when to look again.
pub fn watch(d: Arc<Daemon>) {
    let mut events = d.events.subscribe();
    tokio::spawn(async move {
        reconcile(&d);
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = tick.tick() => reconcile(&d),
                event = events.recv() => match event {
                    Ok(e) if e.kind == "status" || e.kind == "gateway_state" || e.kind == "run_started" || e.kind == "attention" => reconcile(&d),
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => reconcile(&d),
                    Err(_) => return,
                },
            }
        }
    });
}
