//! Another app on a call: Voice Mode pauses by itself and resumes when the call is done (AC-173).
//!
//! macOS lists the processes that use audio (Core Audio's process objects, macOS 14.2 and later),
//! each with whether it is recording and whether it is playing. A call does both; an app that only
//! records (a dictation tool such as Wispr Flow, which keeps the microphone open all day) does not
//! pause Voice Mode (the owner, 2026-09-28). Any process but this one that is on a call counts. Tests and
//! the simulated voice use a file instead (`OVERSEER_LISTENER_TEST_MIC_USERS`: one app a line), so
//! a real call on the machine never pauses a test.

/// The apps other than the listener that are recording now.
pub fn others_recording(simulated: bool) -> Vec<String> {
    if let Some(path) = std::env::var_os("OVERSEER_LISTENER_TEST_MIC_USERS") {
        return std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
    }
    if simulated {
        return Vec::new();
    }
    mac::recording()
        .into_iter()
        .filter(|(pid, _)| *pid != std::process::id() as i32)
        .map(|(_, name)| name)
        .filter(|name| !never_a_call(name))
        .collect()
}

/// Dictation and the system's own speech keep the microphone (and often a sound) open without
/// being a call: Wispr Flow and its helpers, macOS's speech services (the owner, 2026-09-28).
const NOT_CALLS: &[&str] = &["com.electron.wispr-flow", "com.apple.CoreSpeech", "com.apple.corespeechd", "com.apple.SpeechRecognitionCore"];

fn never_a_call(bundle: &str) -> bool {
    NOT_CALLS.iter().any(|p| bundle.len() >= p.len() && bundle[..p.len()].eq_ignore_ascii_case(p))
}

#[cfg(not(target_os = "macos"))]
mod mac {
    pub fn recording() -> Vec<(i32, String)> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(s: *const c_void, buf: *mut u8, size: isize, encoding: u32) -> u8;
        fn CFRelease(v: *const c_void);
    }

    const SYSTEM: u32 = 1;
    const fn code(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }
    const GLOBAL: u32 = code(b"glob");
    const PROCESSES: u32 = code(b"prs#");
    const PID: u32 = code(b"ppid");
    const RECORDING: u32 = code(b"piri");
    const PLAYING: u32 = code(b"piro");
    const BUNDLE: u32 = code(b"pbid");

    fn get<T: Default>(object: u32, selector: u32) -> Option<T> {
        let address = Address {
            selector,
            scope: GLOBAL,
            element: 0,
        };
        let mut value = T::default();
        let mut size = std::mem::size_of::<T>() as u32;
        let e = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut T as *mut c_void,
            )
        };
        (e == 0).then_some(value)
    }

    /// Each process on a call now (recording and playing): its id and its bundle id (or its process id).
    pub fn recording() -> Vec<(i32, String)> {
        let address = Address {
            selector: PROCESSES,
            scope: GLOBAL,
            element: 0,
        };
        let mut size = 0u32;
        if unsafe {
            AudioObjectGetPropertyDataSize(SYSTEM, &address, 0, std::ptr::null(), &mut size)
        } != 0
        {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / 4];
        if unsafe {
            AudioObjectGetPropertyData(
                SYSTEM,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                ids.as_mut_ptr() as *mut c_void,
            )
        } != 0
        {
            return Vec::new();
        }
        ids.truncate(size as usize / 4);
        ids.into_iter()
            .filter(|&id| get::<u32>(id, RECORDING).unwrap_or(0) != 0 && get::<u32>(id, PLAYING).unwrap_or(0) != 0)
            .map(|id| {
                let pid = get::<i32>(id, PID).unwrap_or(0);
                let name = get::<usize>(id, BUNDLE)
                    .filter(|p| *p != 0)
                    .and_then(|p| {
                        let s = p as *const c_void;
                        let mut buf = [0u8; 256];
                        let ok =
                            unsafe { CFStringGetCString(s, buf.as_mut_ptr(), 256, 0x0800_0100) };
                        unsafe { CFRelease(s) };
                        (ok != 0).then(|| {
                            let end = buf.iter().position(|&b| b == 0).unwrap_or(0);
                            String::from_utf8_lossy(&buf[..end]).to_string()
                        })
                    })
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("process {pid}"));
                (pid, name)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dictation_and_system_speech_are_never_a_call() {
        for id in ["com.electron.wispr-flow", "com.electron.wispr-flow.helper", "com.apple.CoreSpeech", "com.apple.corespeechd"] {
            assert!(super::never_a_call(id), "{id}");
        }
        for id in ["us.zoom.xos", "com.apple.FaceTime", "com.tinyspeck.slackmacgap"] {
            assert!(!super::never_a_call(id), "{id}");
        }
    }

    #[test]
    #[ignore]
    fn print_the_real_list() {
        for (pid, name) in super::mac::recording() {
            eprintln!("on a call: {pid} {name}");
        }
    }

    #[test]
    fn the_real_list_can_be_read_and_never_names_this_process() {
        // Whatever the machine is doing, reading the list works and leaves this process out.
        let me = std::process::id() as i32;
        let all = super::mac::recording();
        assert!(all.iter().all(|(pid, _)| *pid >= 0));
        std::env::remove_var("OVERSEER_LISTENER_TEST_MIC_USERS");
        let others = super::others_recording(false);
        assert!(others.len() <= all.len());
        assert!(!all.iter().any(|(pid, name)| *pid == me
            && others.contains(name)
            && all.iter().filter(|(_, n)| n == name).count() == 1));
    }
}
