//! Overseer's voice made in memory (AC-173: audio is never written to disk). macOS's speech
//! synthesizer writes into an audio "file" whose reads and writes are callbacks on a buffer here,
//! so no file exists at any point. The same system voices as `say`.

#![allow(non_upper_case_globals, non_snake_case)]

use anyhow::{bail, Result};
use std::ffi::c_void;
use std::time::{Duration, Instant};

type OSStatus = i32;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFNumberRef = *const c_void;
type AudioFileID = *mut c_void;
type ExtAudioFileRef = *mut c_void;
type SpeechChannel = *mut c_void;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Asbd {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C, packed(2))]
#[derive(Default, Clone, Copy)]
struct VoiceSpec {
    creator: u32,
    id: u32,
}

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
struct VoiceDescription {
    length: i32,
    voice: VoiceSpec,
    version: u32,
    name: [u8; 64],
    comment: [u8; 256],
    gender: i16,
    age: i16,
    script: i16,
    language: i16,
    region: i16,
    reserved: [i32; 4],
}

type ReadProc = extern "C" fn(*mut c_void, i64, u32, *mut c_void, *mut u32) -> OSStatus;
type WriteProc = extern "C" fn(*mut c_void, i64, u32, *const c_void, *mut u32) -> OSStatus;
type GetSizeProc = extern "C" fn(*mut c_void) -> i64;
type SetSizeProc = extern "C" fn(*mut c_void, i64) -> OSStatus;

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioFileInitializeWithCallbacks(
        client: *mut c_void,
        read: ReadProc,
        write: WriteProc,
        get_size: GetSizeProc,
        set_size: SetSizeProc,
        file_type: u32,
        format: *const Asbd,
        flags: u32,
        out: *mut AudioFileID,
    ) -> OSStatus;
    fn AudioFileClose(file: AudioFileID) -> OSStatus;
    fn ExtAudioFileWrapAudioFileID(
        file: AudioFileID,
        for_writing: u8,
        out: *mut ExtAudioFileRef,
    ) -> OSStatus;
    fn ExtAudioFileDispose(file: ExtAudioFileRef) -> OSStatus;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kSpeechOutputToExtAudioFileProperty: CFStringRef;
    static kSpeechRateProperty: CFStringRef;
    fn NewSpeechChannel(voice: *const VoiceSpec, out: *mut SpeechChannel) -> OSStatus;
    fn DisposeSpeechChannel(chan: SpeechChannel) -> OSStatus;
    fn SetSpeechProperty(chan: SpeechChannel, property: CFStringRef, value: CFTypeRef) -> OSStatus;
    fn SpeakCFString(chan: SpeechChannel, text: CFStringRef, options: CFTypeRef) -> OSStatus;
    fn StopSpeech(chan: SpeechChannel) -> OSStatus;
    fn SpeechBusy() -> i16;
    fn CountVoices(n: *mut i16) -> OSStatus;
    fn GetIndVoice(index: i16, voice: *mut VoiceSpec) -> OSStatus;
    fn GetVoiceDescription(
        voice: *const VoiceSpec,
        info: *mut VoiceDescription,
        len: i32,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFNumberCreate(alloc: CFTypeRef, kind: isize, value: *const c_void) -> CFNumberRef;
    fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> CFStringRef;
    fn CFRelease(v: CFTypeRef);
}

const K_WAVE: u32 = u32::from_be_bytes(*b"WAVE");
const K_LPCM: u32 = u32::from_be_bytes(*b"lpcm");
const SIGNED_PACKED: u32 = 0x4 | 0x8;
const CF_NUMBER_SINT64: isize = 4;
const CF_NUMBER_FLOAT64: isize = 13;
const UTF8: u32 = 0x0800_0100;

/// The buffer the synthesizer's "file" lives in.
#[derive(Default)]
struct Mem {
    buf: Vec<u8>,
}

extern "C" fn read_cb(
    c: *mut c_void,
    pos: i64,
    n: u32,
    out: *mut c_void,
    got: *mut u32,
) -> OSStatus {
    let m = unsafe { &*(c as *const Mem) };
    let pos = pos.max(0) as usize;
    let k = m.buf.len().saturating_sub(pos).min(n as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(m.buf.as_ptr().add(pos), out as *mut u8, k);
        *got = k as u32;
    }
    0
}

extern "C" fn write_cb(
    c: *mut c_void,
    pos: i64,
    n: u32,
    data: *const c_void,
    done: *mut u32,
) -> OSStatus {
    let m = unsafe { &mut *(c as *mut Mem) };
    let pos = pos.max(0) as usize;
    let end = pos + n as usize;
    if m.buf.len() < end {
        m.buf.resize(end, 0);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(data as *const u8, m.buf.as_mut_ptr().add(pos), n as usize);
        *done = n;
    }
    0
}

extern "C" fn size_cb(c: *mut c_void) -> i64 {
    unsafe { (*(c as *const Mem)).buf.len() as i64 }
}

extern "C" fn set_size_cb(c: *mut c_void, size: i64) -> OSStatus {
    unsafe { (*(c as *mut Mem)).buf.resize(size.max(0) as usize, 0) };
    0
}

/// The system voice called `name` ("Samantha"), as the speech synthesizer knows it.
fn voice_named(name: &str) -> Option<VoiceSpec> {
    let mut n: i16 = 0;
    if unsafe { CountVoices(&mut n) } != 0 {
        return None;
    }
    for i in 1..=n {
        let mut spec = VoiceSpec::default();
        if unsafe { GetIndVoice(i, &mut spec) } != 0 {
            continue;
        }
        let mut d: VoiceDescription = unsafe { std::mem::zeroed() };
        let len = std::mem::size_of::<VoiceDescription>() as i32;
        if unsafe { GetVoiceDescription(&spec, &mut d, len) } != 0 {
            continue;
        }
        let l = (d.name[0] as usize).min(63);
        let got = String::from_utf8_lossy(&d.name[1..1 + l]).to_string();
        if got.eq_ignore_ascii_case(name) {
            return Some(spec);
        }
    }
    None
}

/// Speaks `text` into 16 kHz mono samples, in memory. A time limit that grows with the text (the
/// speech service sometimes never finishes on a busy Mac); the caller tries once more.
pub fn speak(text: &str, voice: Option<&str>, rate: Option<u32>) -> Result<Vec<f32>> {
    let mut mem = Box::new(Mem::default());
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
    let client = &mut *mem as *mut Mem as *mut c_void;
    let mut file: AudioFileID = std::ptr::null_mut();
    let e = unsafe {
        AudioFileInitializeWithCallbacks(
            client,
            read_cb,
            write_cb,
            size_cb,
            set_size_cb,
            K_WAVE,
            &format,
            0,
            &mut file,
        )
    };
    if e != 0 {
        bail!("the in-memory audio file could not be made ({e})");
    }
    let mut ext: ExtAudioFileRef = std::ptr::null_mut();
    let e = unsafe { ExtAudioFileWrapAudioFileID(file, 1, &mut ext) };
    if e != 0 {
        unsafe { AudioFileClose(file) };
        bail!("the in-memory audio file could not be wrapped ({e})");
    }
    let spec = voice.and_then(voice_named);
    let mut chan: SpeechChannel = std::ptr::null_mut();
    let e = unsafe {
        NewSpeechChannel(
            spec.as_ref()
                .map_or(std::ptr::null(), |s| s as *const VoiceSpec),
            &mut chan,
        )
    };
    let close = |chan: SpeechChannel| unsafe {
        if !chan.is_null() {
            DisposeSpeechChannel(chan);
        }
        ExtAudioFileDispose(ext);
        AudioFileClose(file);
    };
    if e != 0 {
        close(std::ptr::null_mut());
        bail!("no speech channel ({e})");
    }
    let result = (|| -> Result<()> {
        let ptr = ext as i64;
        let num = unsafe {
            CFNumberCreate(
                std::ptr::null(),
                CF_NUMBER_SINT64,
                &ptr as *const i64 as *const c_void,
            )
        };
        let e = unsafe { SetSpeechProperty(chan, kSpeechOutputToExtAudioFileProperty, num) };
        unsafe { CFRelease(num) };
        if e != 0 {
            bail!("the speech output could not be set ({e})");
        }
        if let Some(r) = rate {
            let wpm = r as f64;
            let num = unsafe {
                CFNumberCreate(
                    std::ptr::null(),
                    CF_NUMBER_FLOAT64,
                    &wpm as *const f64 as *const c_void,
                )
            };
            unsafe { SetSpeechProperty(chan, kSpeechRateProperty, num) };
            unsafe { CFRelease(num) };
        }
        let s = unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                text.as_ptr(),
                text.len() as isize,
                UTF8,
                0,
            )
        };
        let e = unsafe { SpeakCFString(chan, s, std::ptr::null()) };
        unsafe { CFRelease(s) };
        if e != 0 {
            bail!("the line could not be spoken ({e})");
        }
        let limit = Duration::from_millis(20_000 + 100 * text.chars().count() as u64);
        let started = Instant::now();
        std::thread::sleep(Duration::from_millis(20));
        while unsafe { SpeechBusy() } > 0 {
            if started.elapsed() > limit {
                unsafe { StopSpeech(chan) };
                bail!("the speech synthesizer did not finish in time");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    close(chan);
    result?;
    crate::pcm::read_wav(&mem.buf)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_line_is_made_in_memory_with_no_file() {
        let before: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        // The speech service can time out on a busy Mac; callers try once more, and so does this test.
        let line = "On it. Telling Phone to wait.";
        let audio = super::speak(line, None, None).or_else(|_| super::speak(line, None, None)).unwrap();
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
            "no temporary file: {after:?}"
        );
    }
}

#[cfg(test)]
mod voices {
    #[test]
    fn a_voice_is_found_by_the_name_say_shows() {
        let out = std::process::Command::new("say")
            .args(["-v", "?"])
            .output()
            .unwrap();
        let first = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .and_then(|l| l.split("  ").next())
            .map(|n| n.trim().to_string())
            .unwrap_or_default();
        assert!(!first.is_empty());
        assert!(super::voice_named(&first).is_some(), "{first}");
        assert!(super::voice_named("No Such Voice").is_none());
    }
}
