//! One-attempt diagnostic variant, not the ordinary test's Err-only retry.
//! Never linked into production. No input text or raw PCM is emitted.
//! Callback IDs refer to retained slots, not borrowed/stack pointers. Slots are never reused or
//! freed before worker process exit: disposal is not assumed to quiesce late callbacks.

use super::*;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_SLOTS: usize = 4;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_EVENTS: usize = 64;
const MAX_OUTPUT: usize = 64 * 1024;
const TEST: &str = "memspeech::diagnostic::one_native_attempt_has_complete_in_memory_speech";
const WORKER: &str = "OVERSEER_MEMSPEECH_DIAGNOSTIC_WORKER";

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kSpeechRefConProperty: CFStringRef;
    static kSpeechSpeechDoneCallBack: CFStringRef;
    static kSpeechErrorCFCallBack: CFStringRef;
    static kSpeechCurrentVoiceProperty: CFStringRef;
    static kSpeechVoiceCreator: CFStringRef;
    static kSpeechVoiceID: CFStringRef;
    static kSpeechStatusProperty: CFStringRef;
    static kSpeechStatusOutputBusy: CFStringRef;
    static kSpeechStatusOutputPaused: CFStringRef;
    static kSpeechStatusNumberOfCharactersLeft: CFStringRef;
    static kSpeechErrorsProperty: CFStringRef;
    // SpeechSynthesis.h: OSErr (signed 16 bits), unlike AudioToolbox's OSStatus.
    fn CopySpeechProperty(chan: SpeechChannel, key: CFStringRef, out: *mut CFTypeRef) -> i16;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFGetTypeID(object: CFTypeRef) -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryGetCount(object: CFTypeRef) -> isize;
    fn CFDictionaryGetValue(object: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(object: CFTypeRef, kind: isize, out: *mut c_void) -> u8;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(object: CFTypeRef) -> u8;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(object: CFTypeRef) -> isize;
    fn CFErrorGetCode(error: CFTypeRef) -> isize;
}

#[derive(Default)]
struct Trace {
    events: VecDeque<Value>,
    omitted: u64,
    emitted: usize,
    writes: u64,
    written_bytes: u64,
    first_write_us: Option<u64>,
    last_write_us: Option<u64>,
    first_write_position: Option<usize>,
    last_write_position: Option<usize>,
    largest_write_end: usize,
    done_callbacks: u64,
    error_callbacks: u64,
    capped: bool,
}

struct Slot {
    started: Instant,
    mem: Mutex<Mem>,
    trace: Mutex<Trace>,
}

impl Slot {
    fn us(&self) -> u64 {
        self.started.elapsed().as_micros().min(u64::MAX as u128) as u64
    }

    fn event(&self, kind: &'static str, detail: Value) {
        let mut trace = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        if trace.events.len() == MAX_EVENTS {
            trace.events.pop_front();
            trace.omitted += 1;
        }
        let event = json!({"us":self.us(), "kind":kind, "detail":detail});
        // At most 64 progress records plus one bounded snapshot. Progress survives a hung
        // native call; no callback/file locks depend on the parent reader.
        if trace.emitted < MAX_EVENTS {
            eprintln!("MEMSPEECH_DIAGNOSTIC_EVENT {event}");
            trace.emitted += 1;
        }
        trace.events.push_back(event);
    }

    fn status(&self, phase: &'static str, status: OSStatus) {
        self.event(phase, json!({"status":status}));
    }

    fn snapshot(&self, bytes: &[u8], audio: Option<&[f32]>) -> Value {
        let trace = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        json!({"variant":"one_attempt_default_voice", "events":trace.events,
            "omitted_events":trace.omitted, "writes":trace.writes,
            "callback_bytes":trace.written_bytes, "first_write_us":trace.first_write_us,
            "last_write_us":trace.last_write_us, "first_write_position":trace.first_write_position,
            "last_write_position":trace.last_write_position, "largest_write_end":trace.largest_write_end,
            "done_callbacks":trace.done_callbacks,
            "error_callbacks":trace.error_callbacks, "buffer_cap_hit":trace.capped,
            "wave":wave_metadata(bytes), "samples":audio.map(|a|a.len()),
            "peak":audio.map(|a|a.iter().fold(0.0_f32,|p,s|p.max(s.abs()))),
            "snapshot_us":self.us(), "late_callbacks_safe_until_worker_exit":true})
    }
}

// Process lifetime retention is deliberate and bounded (four slots, at most 8 MiB of audio).
// File clients and LP64 SRefCon are opaque IDs, never dereferenced as addresses.
static SLOTS: OnceLock<Mutex<Vec<Arc<Slot>>>> = OnceLock::new();

fn allocate() -> Result<(usize, Arc<Slot>)> {
    let mut slots = SLOTS.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if slots.len() == MAX_SLOTS {
        bail!("diagnostic retained slot limit reached");
    }
    let slot = Arc::new(Slot {
        started: Instant::now(),
        mem: Mutex::new(Mem::default()),
        trace: Mutex::new(Trace::default()),
    });
    slots.push(slot.clone());
    Ok((slots.len(), slot))
}

fn lookup(client: *mut c_void) -> Option<Arc<Slot>> {
    let index = (client as usize).checked_sub(1)?;
    SLOTS
        .get()?
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(index)
        .cloned()
}

extern "C" fn read(
    client: *mut c_void,
    pos: i64,
    n: u32,
    out: *mut c_void,
    got: *mut u32,
) -> OSStatus {
    let Some(slot) = lookup(client) else {
        return -1;
    };
    let mem = slot.mem.lock().unwrap_or_else(|e| e.into_inner());
    let pos = pos.max(0) as usize;
    let k = mem.buf.len().saturating_sub(pos).min(n as usize);
    unsafe {
        if k > 0 {
            std::ptr::copy_nonoverlapping(mem.buf.as_ptr().add(pos), out.cast(), k);
        }
        *got = k as u32;
    }
    0
}

extern "C" fn write(
    client: *mut c_void,
    pos: i64,
    n: u32,
    data: *const c_void,
    done: *mut u32,
) -> OSStatus {
    let Some(slot) = lookup(client) else {
        return -1;
    };
    let pos = pos.max(0) as usize;
    let Some(end) = pos.checked_add(n as usize).filter(|end| *end <= MAX_BYTES) else {
        slot.trace.lock().unwrap_or_else(|e| e.into_inner()).capped = true;
        unsafe {
            *done = 0;
        }
        return -1;
    };
    {
        let mut mem = slot.mem.lock().unwrap_or_else(|e| e.into_inner());
        if mem.buf.len() < end {
            mem.buf.resize(end, 0);
        }
        unsafe {
            if n > 0 {
                std::ptr::copy_nonoverlapping(
                    data.cast(),
                    mem.buf.as_mut_ptr().add(pos),
                    n as usize,
                );
            }
            *done = n;
        }
    }
    let mut trace = slot.trace.lock().unwrap_or_else(|e| e.into_inner());
    trace.writes = trace.writes.saturating_add(1);
    trace.written_bytes = trace.written_bytes.saturating_add(u64::from(n));
    trace.first_write_position.get_or_insert(pos);
    trace.last_write_position = Some(pos);
    trace.largest_write_end = trace.largest_write_end.max(end);
    trace.first_write_us.get_or_insert(slot.us());
    trace.last_write_us = Some(slot.us());
    // Aggregate writes keep polling/completion/disposal events in the bounded ring.
    0
}

extern "C" fn size(client: *mut c_void) -> i64 {
    lookup(client).map_or(0, |slot| {
        slot.mem.lock().unwrap_or_else(|e| e.into_inner()).buf.len() as i64
    })
}

extern "C" fn set_size(client: *mut c_void, n: i64) -> OSStatus {
    let Some(slot) = lookup(client) else {
        return -1;
    };
    let n = n.max(0) as usize;
    if n > MAX_BYTES {
        slot.trace.lock().unwrap_or_else(|e| e.into_inner()).capped = true;
        return -1;
    }
    slot.mem
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .buf
        .resize(n, 0);
    slot.event("set_size", json!({"bytes":n}));
    0
}

// LP64 SRefCon is void*, verified from the installed SDK's MacTypes.h.
extern "C" fn speech_done(_chan: SpeechChannel, refcon: *mut c_void) {
    if let Some(slot) = lookup(refcon) {
        slot.trace
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .done_callbacks += 1;
        slot.event("channel_done_callback", json!({}));
    }
}

extern "C" fn speech_error(_chan: SpeechChannel, refcon: *mut c_void, error: CFTypeRef) {
    if let Some(slot) = lookup(refcon) {
        slot.trace
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .error_callbacks += 1;
        let code = if error.is_null() {
            None
        } else {
            Some(unsafe { CFErrorGetCode(error) })
        };
        // Error userInfo/descriptions can contain input; only the numeric code is retained.
        slot.event("channel_error_callback", json!({"code":code}));
    }
}

fn set_pointer(chan: SpeechChannel, key: CFStringRef, ptr: usize) -> i16 {
    let ptr = ptr as i64;
    unsafe {
        let number = CFNumberCreate(
            std::ptr::null(),
            CF_NUMBER_SINT64,
            (&ptr as *const i64).cast(),
        );
        if number.is_null() {
            return -1;
        }
        let status = SetSpeechProperty(chan, key, number) as i16;
        CFRelease(number);
        status
    }
}

fn number(value: CFTypeRef) -> Option<i64> {
    if value.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(value) == CFBooleanGetTypeID() {
            return Some(i64::from(CFBooleanGetValue(value)));
        }
        if CFGetTypeID(value) != CFNumberGetTypeID() {
            return None;
        }
        let mut n = 0_i64;
        (CFNumberGetValue(value, CF_NUMBER_SINT64, (&mut n as *mut i64).cast()) != 0).then_some(n)
    }
}

fn inspect_channel(slot: &Slot, chan: SpeechChannel, phase: &'static str) {
    unsafe {
        for (label, property, keys) in [
            (
                "status",
                kSpeechStatusProperty,
                vec![
                    ("busy", kSpeechStatusOutputBusy),
                    ("paused", kSpeechStatusOutputPaused),
                    ("remaining", kSpeechStatusNumberOfCharactersLeft),
                ],
            ),
            (
                "voice",
                kSpeechCurrentVoiceProperty,
                vec![("creator", kSpeechVoiceCreator), ("id", kSpeechVoiceID)],
            ),
            ("errors", kSpeechErrorsProperty, vec![]),
        ] {
            let mut value = std::ptr::null();
            slot.event(
                "copy_property_enter",
                json!({"phase":phase,"property":label}),
            );
            let status = CopySpeechProperty(chan, property, &mut value);
            let mut receipt = json!({"property":label,"status":status});
            if status == 0 && !value.is_null() {
                let kind = CFGetTypeID(value);
                if kind == CFDictionaryGetTypeID() {
                    receipt["entries"] = json!(CFDictionaryGetCount(value));
                    for (name, key) in keys {
                        receipt[name] = json!(number(CFDictionaryGetValue(value, key)));
                    }
                    if label == "voice" {
                        if let (Some(creator), Some(id)) =
                            (receipt["creator"].as_i64(), receipt["id"].as_i64())
                        {
                            let spec = VoiceSpec {
                                creator: creator as u32,
                                id: id as u32,
                            };
                            let mut desc: VoiceDescription = std::mem::zeroed();
                            receipt["description_status"] = json!(GetVoiceDescription(
                                &spec,
                                &mut desc,
                                std::mem::size_of::<VoiceDescription>() as i32
                            )
                                as i16);
                        }
                    }
                } else if kind == CFArrayGetTypeID() {
                    receipt["entries"] = json!(CFArrayGetCount(value));
                }
            }
            // Copy contract returns retained objects; dictionary members are borrowed.
            if !value.is_null() {
                CFRelease(value);
            }
            slot.event(phase, receipt);
        }
    }
}

fn wave_metadata(bytes: &[u8]) -> Value {
    let u32_at = |p: usize| {
        bytes
            .get(p..p + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let mut chunks = Vec::new();
    let mut truncated = false;
    let mut data_chunks = 0;
    let mut at = 12_usize;
    while at.saturating_add(8) <= bytes.len() && chunks.len() < 16 {
        let declared = u32_at(at + 4).unwrap() as usize;
        let available = bytes.len().saturating_sub(at + 8).min(declared);
        truncated |= available < declared;
        let tag = &bytes[at..at + 4];
        if tag == b"data" {
            data_chunks += 1;
        }
        // Only known chunk labels are printed, never arbitrary content-bearing bytes.
        let label = if tag == b"fmt " {
            "fmt"
        } else if tag == b"data" {
            "data"
        } else {
            "other"
        };
        let mut chunk = json!({"kind":label,"offset":at,"declared":declared,"available":available});
        if tag == b"fmt " && available >= 16 {
            let body = &bytes[at + 8..at + 24];
            chunk["format"] = json!(u16::from_le_bytes(body[0..2].try_into().unwrap()));
            chunk["channels"] = json!(u16::from_le_bytes(body[2..4].try_into().unwrap()));
            chunk["rate"] = json!(u32::from_le_bytes(body[4..8].try_into().unwrap()));
            chunk["bits"] = json!(u16::from_le_bytes(body[14..16].try_into().unwrap()));
        }
        chunks.push(chunk);
        let Some(next) = at
            .checked_add(8)
            .and_then(|v| v.checked_add(declared))
            .and_then(|v| v.checked_add(declared & 1))
        else {
            break;
        };
        at = next;
    }
    json!({"bytes":bytes.len(),"riff":bytes.get(0..4)==Some(b"RIFF"),
        "wave":bytes.get(8..12)==Some(b"WAVE"),"riff_declared":u32_at(4),
        "riff_declared_exceeds_buffer":u32_at(4).map(|n|u64::from(n)+8>bytes.len() as u64),
        "truncated_chunk_body":truncated,"data_chunks_seen":data_chunks,"chunks":chunks})
}

fn attempt(slot: &Slot, id: usize) -> Result<Vec<f32>> {
    // Fixed synthetic phrase, same format/default voice/rate and polling/disposal order as speak.
    let text = "On it. Telling Phone to wait.";
    let format = Asbd {
        sample_rate: 16_000.0,
        format_id: K_LPCM,
        format_flags: SIGNED_PACKED,
        bytes_per_packet: 2,
        frames_per_packet: 1,
        bytes_per_frame: 2,
        channels_per_frame: 1,
        bits_per_channel: 16,
        reserved: 0,
    };
    let mut file = std::ptr::null_mut();
    slot.event("initialize_file_enter", json!({}));
    let e = unsafe {
        AudioFileInitializeWithCallbacks(
            id as *mut c_void,
            read,
            write,
            size,
            set_size,
            K_WAVE,
            &format,
            0,
            &mut file,
        )
    };
    slot.status("initialize_file", e);
    if e != 0 {
        bail!("diagnostic initialization failed");
    }
    let mut ext = std::ptr::null_mut();
    slot.event("wrap_file_enter", json!({}));
    let e = unsafe { ExtAudioFileWrapAudioFileID(file, 1, &mut ext) };
    slot.status("wrap_file", e);
    if e != 0 {
        slot.status("close_file", unsafe { AudioFileClose(file) });
        bail!("diagnostic wrapping failed");
    }
    let mut chan = std::ptr::null_mut();
    slot.event("new_channel_enter", json!({}));
    let e = unsafe { NewSpeechChannel(std::ptr::null(), &mut chan) } as i16;
    slot.status("new_channel", i32::from(e));
    let close = |chan: SpeechChannel| {
        if !chan.is_null() {
            slot.event("dispose_channel_enter", json!({}));
            slot.status(
                "dispose_channel_return",
                unsafe { DisposeSpeechChannel(chan) } as i16 as i32,
            );
        }
        slot.event("dispose_ext_enter", json!({}));
        slot.status("dispose_ext_return", unsafe { ExtAudioFileDispose(ext) });
        slot.event("close_file_enter", json!({}));
        slot.status("close_file_return", unsafe { AudioFileClose(file) });
    };
    if e != 0 {
        close(std::ptr::null_mut());
        bail!("diagnostic channel failed");
    }
    let result = (|| -> Result<()> {
        for (phase, key, pointer) in unsafe {
            [
                ("set_refcon", kSpeechRefConProperty, id),
                (
                    "set_done_callback",
                    kSpeechSpeechDoneCallBack,
                    speech_done as *const () as usize,
                ),
                (
                    "set_error_callback",
                    kSpeechErrorCFCallBack,
                    speech_error as *const () as usize,
                ),
                (
                    "set_output",
                    kSpeechOutputToExtAudioFileProperty,
                    ext as usize,
                ),
            ]
        } {
            slot.event("set_property_enter", json!({"phase":phase}));
            let e = set_pointer(chan, key, pointer);
            slot.status(phase, i32::from(e));
            if e != 0 {
                bail!("diagnostic observer/output setup failed");
            }
        }
        inspect_channel(slot, chan, "before_speak");
        let s = unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                text.as_ptr(),
                text.len() as isize,
                UTF8,
                0,
            )
        };
        if s.is_null() {
            bail!("diagnostic text allocation failed");
        }
        slot.event("speak_enter", json!({"characters":text.chars().count()}));
        let e = unsafe { SpeakCFString(chan, s, std::ptr::null()) } as i16;
        unsafe {
            CFRelease(s);
        }
        slot.status("speak_return", i32::from(e));
        if e != 0 {
            bail!("diagnostic speak failed");
        }
        let limit = Duration::from_millis(20_000 + 100 * text.chars().count() as u64);
        let started = Instant::now();
        std::thread::sleep(Duration::from_millis(20));
        let mut previous = None;
        loop {
            let busy = unsafe { SpeechBusy() };
            if previous != Some(busy) {
                slot.event("global_busy", json!({"value":busy}));
            }
            previous = Some(busy);
            if busy <= 0 {
                break;
            }
            if started.elapsed() > limit {
                slot.status("stop_on_timeout", unsafe { StopSpeech(chan) } as i16 as i32);
                bail!("diagnostic original polling deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        inspect_channel(slot, chan, "before_dispose");
        Ok(())
    })();
    close(chan);
    result?;
    let bytes = slot
        .mem
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .buf
        .clone();
    crate::pcm::read_wav(&bytes)
}

struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "explicit native diagnostic allocation; one attempt, 35-second worker watchdog"]
fn one_native_attempt_has_complete_in_memory_speech() {
    if std::env::var(WORKER).as_deref() != Ok("1") {
        let mut child = Reap(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    TEST,
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(WORKER, "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("diagnostic worker starts"),
        );
        let stderr = child.0.stderr.take().unwrap();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stderr.take((MAX_OUTPUT + 1) as u64).read_to_end(&mut bytes);
            let _ = send.send((result.is_ok(), bytes));
        });
        let deadline = Instant::now() + Duration::from_secs(35);
        let status = loop {
            if let Some(status) = child.0.try_wait().expect("diagnostic worker status") {
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        if status.is_none() {
            let _ = child.0.kill();
            let _ = child.0.wait();
        }
        let (read_ok, bytes) = receive
            .recv_timeout(Duration::from_secs(1))
            .expect("bounded worker output");
        // Emit only the bounded metadata marker, never arbitrary child diagnostics/input.
        for line in String::from_utf8_lossy(&bytes).lines().filter(|line| {
            line.starts_with("MEMSPEECH_DIAGNOSTIC ")
                || line.starts_with("MEMSPEECH_DIAGNOSTIC_EVENT ")
        }) {
            eprintln!("{line}");
        }
        assert!(
            read_ok && bytes.len() <= MAX_OUTPUT,
            "diagnostic output bound"
        );
        assert!(status.is_some(), "diagnostic worker exceeded 35 seconds");
        assert!(
            status.unwrap().success(),
            "diagnostic worker failed; see bounded metadata"
        );
        return;
    }
    let before: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    let (id, slot) = allocate().expect("bounded retained diagnostic state");
    let audio = attempt(&slot, id); // Exactly one attempt; no Err retry in this variant.
    let bytes = slot
        .mem
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .buf
        .clone();
    eprintln!(
        "MEMSPEECH_DIAGNOSTIC {}",
        slot.snapshot(&bytes, audio.as_deref().ok())
    );
    let audio = audio.expect("one-attempt native diagnostic returns audio");
    assert!(audio.len() > 16_000, "{} samples", audio.len());
    assert!(audio.iter().any(|s| s.abs() > 0.05), "it has sound");
    let after: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .filter(|n| !before.contains(n))
        .collect();
    assert!(
        !after
            .iter()
            .any(|n| n.to_string_lossy().starts_with("ovs-voice-")),
        "no temporary audio file"
    );
}

#[test]
fn retained_callbacks_and_metadata_are_bounded_without_native_speech() {
    let (id, slot) = allocate().unwrap();
    let client = id as *mut c_void;
    // Drop the attempt's owner before simulated late callbacks. The registry owns the slot.
    drop(slot);
    speech_done(std::ptr::null_mut(), client);
    speech_error(std::ptr::null_mut(), client, std::ptr::null());
    let data = [1_u8, 2, 3, 4];
    let mut done = 0;
    assert_eq!(write(client, 0, 4, data.as_ptr().cast(), &mut done), 0);
    assert_eq!(done, 4);
    assert_eq!(size(client), 4);
    let mut got = 0;
    let mut copy = [0_u8; 4];
    assert_eq!(read(client, 0, 4, copy.as_mut_ptr().cast(), &mut got), 0);
    assert_eq!(copy, data);
    assert_eq!(got, 4);
    assert_eq!(set_size(client, MAX_BYTES as i64 + 1), -1);
    assert_eq!(
        write(client, MAX_BYTES as i64, 4, data.as_ptr().cast(), &mut done),
        -1
    );
    assert_eq!(done, 0);
    let slot = lookup(client).unwrap();
    for _ in 0..MAX_EVENTS + 2 {
        slot.event("synthetic", json!({}));
    }
    let bytes = slot.mem.lock().unwrap().buf.clone();
    let receipt = slot.snapshot(&bytes, None);
    assert_eq!(receipt["events"].as_array().unwrap().len(), MAX_EVENTS);
    assert!(receipt["omitted_events"].as_u64().unwrap() >= 2);
    assert_eq!(receipt["done_callbacks"], 1);
    assert_eq!(receipt["error_callbacks"], 1);
    assert_eq!(receipt["buffer_cap_hit"], true);
    assert_eq!(receipt["wave"]["bytes"], 4);
    assert_eq!(size(client), 4);
    assert!(lookup(std::ptr::null_mut()).is_none());
    assert!(lookup((MAX_SLOTS + 1) as *mut c_void).is_none());
}
