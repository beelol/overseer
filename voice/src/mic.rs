//! The microphone and the speakers, through macOS's voice-processing I/O unit (AC-162, AC-163).
//!
//! One audio unit does both: it records the microphone at 16 kHz and plays Overseer's voice, and
//! because it knows what it plays it cancels that from what it records (echo cancellation). With
//! `voice_processing` off the same unit runs with its processing bypassed.
//!
//! Recorded audio stays in memory: it is handed to the listener's loop 10 ms at a time and never
//! written anywhere. macOS asks the owner once for the microphone; the prompt names the app the
//! listener runs in (`Overseer Listener.app`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Keeps the audio unit alive.
pub struct Device {
    #[allow(dead_code)]
    inner: *mut std::ffi::c_void,
}
unsafe impl Send for Device {}

pub type Output = Arc<Mutex<VecDeque<f32>>>;

#[cfg(not(target_os = "macos"))]
pub fn open<F: FnMut(Vec<f32>) + Send + 'static>(
    _voice_processing: bool,
    _on_audio: F,
) -> anyhow::Result<(Device, Output)> {
    anyhow::bail!("the microphone is available on macOS only")
}

#[cfg(target_os = "macos")]
pub use mac::open;

#[cfg(target_os = "macos")]
mod mac {
    #![allow(non_snake_case, non_upper_case_globals)]
    use super::{Device, Output};
    use anyhow::{bail, Result};
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex};

    type OSStatus = i32;
    type AudioUnit = *mut c_void;

    #[repr(C)]
    struct AudioComponentDescription {
        componentType: u32,
        componentSubType: u32,
        componentManufacturer: u32,
        componentFlags: u32,
        componentFlagsMask: u32,
    }

    #[repr(C)]
    struct AudioStreamBasicDescription {
        mSampleRate: f64,
        mFormatID: u32,
        mFormatFlags: u32,
        mBytesPerPacket: u32,
        mFramesPerPacket: u32,
        mBytesPerFrame: u32,
        mChannelsPerFrame: u32,
        mBitsPerChannel: u32,
        mReserved: u32,
    }

    #[repr(C)]
    struct AudioBuffer {
        mNumberChannels: u32,
        mDataByteSize: u32,
        mData: *mut c_void,
    }

    #[repr(C)]
    struct AudioBufferList {
        mNumberBuffers: u32,
        mBuffers: [AudioBuffer; 1],
    }

    type RenderProc = extern "C" fn(
        *mut c_void,
        *mut u32,
        *const c_void,
        u32,
        u32,
        *mut AudioBufferList,
    ) -> OSStatus;

    #[repr(C)]
    struct AURenderCallbackStruct {
        inputProc: RenderProc,
        inputProcRefCon: *mut c_void,
    }

    #[link(name = "AudioToolbox", kind = "framework")]
    extern "C" {
        fn AudioComponentFindNext(
            inComponent: *mut c_void,
            inDesc: *const AudioComponentDescription,
        ) -> *mut c_void;
        fn AudioComponentInstanceNew(
            inComponent: *mut c_void,
            outInstance: *mut AudioUnit,
        ) -> OSStatus;
        fn AudioUnitSetProperty(
            inUnit: AudioUnit,
            inID: u32,
            inScope: u32,
            inElement: u32,
            inData: *const c_void,
            inDataSize: u32,
        ) -> OSStatus;
        fn AudioUnitInitialize(inUnit: AudioUnit) -> OSStatus;
        fn AudioOutputUnitStart(ci: AudioUnit) -> OSStatus;
        fn AudioUnitRender(
            inUnit: AudioUnit,
            ioActionFlags: *mut u32,
            inTimeStamp: *const c_void,
            inOutputBusNumber: u32,
            inNumberFrames: u32,
            ioData: *mut AudioBufferList,
        ) -> OSStatus;
    }

    const kAudioUnitType_Output: u32 = u32::from_be_bytes(*b"auou");
    const kAudioUnitSubType_VoiceProcessingIO: u32 = u32::from_be_bytes(*b"vpio");
    const kAudioUnitManufacturer_Apple: u32 = u32::from_be_bytes(*b"appl");
    const kAudioFormatLinearPCM: u32 = u32::from_be_bytes(*b"lpcm");
    const kAudioFormatFlagIsFloat: u32 = 1;
    const kAudioFormatFlagIsPacked: u32 = 8;
    const kAudioOutputUnitProperty_EnableIO: u32 = 2003;
    const kAudioOutputUnitProperty_SetInputCallback: u32 = 2005;
    const kAudioUnitProperty_StreamFormat: u32 = 8;
    const kAudioUnitProperty_SetRenderCallback: u32 = 23;
    const kAUVoiceIOProperty_BypassVoiceProcessing: u32 = 2100;
    const kAudioUnitScope_Global: u32 = 0;
    const kAudioUnitScope_Input: u32 = 1;
    const kAudioUnitScope_Output: u32 = 2;
    const INPUT_BUS: u32 = 1;
    const OUTPUT_BUS: u32 = 0;

    struct Ctx {
        unit: AudioUnit,
        on_audio: Box<dyn FnMut(Vec<f32>) + Send>,
        output: Output,
        buf: Vec<f32>,
    }

    extern "C" fn recorded(
        refcon: *mut c_void,
        flags: *mut u32,
        ts: *const c_void,
        _bus: u32,
        frames: u32,
        _data: *mut AudioBufferList,
    ) -> OSStatus {
        let ctx = unsafe { &mut *(refcon as *mut Ctx) };
        if ctx.buf.len() < frames as usize {
            ctx.buf.resize(frames as usize, 0.0);
        }
        let mut list = AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: 1,
                mDataByteSize: frames * 4,
                mData: ctx.buf.as_mut_ptr() as *mut c_void,
            }],
        };
        let status = unsafe { AudioUnitRender(ctx.unit, flags, ts, INPUT_BUS, frames, &mut list) };
        if status == 0 {
            (ctx.on_audio)(ctx.buf[..frames as usize].to_vec());
        }
        status
    }

    extern "C" fn play(
        refcon: *mut c_void,
        _flags: *mut u32,
        _ts: *const c_void,
        _bus: u32,
        frames: u32,
        data: *mut AudioBufferList,
    ) -> OSStatus {
        let ctx = unsafe { &mut *(refcon as *mut Ctx) };
        let list = unsafe { &mut *data };
        let out = unsafe {
            std::slice::from_raw_parts_mut(list.mBuffers[0].mData as *mut f32, frames as usize)
        };
        let mut q = match ctx.output.try_lock() {
            Ok(q) => q,
            Err(_) => {
                out.fill(0.0);
                return 0;
            }
        };
        for s in out.iter_mut() {
            *s = q.pop_front().unwrap_or(0.0);
        }
        0
    }

    fn check(status: OSStatus, what: &str) -> Result<()> {
        if status != 0 {
            bail!("{what} failed (OSStatus {status})");
        }
        Ok(())
    }

    pub fn open<F: FnMut(Vec<f32>) + Send + 'static>(
        voice_processing: bool,
        on_audio: F,
    ) -> Result<(Device, Output)> {
        unsafe {
            let desc = AudioComponentDescription {
                componentType: kAudioUnitType_Output,
                componentSubType: kAudioUnitSubType_VoiceProcessingIO,
                componentManufacturer: kAudioUnitManufacturer_Apple,
                componentFlags: 0,
                componentFlagsMask: 0,
            };
            let comp = AudioComponentFindNext(std::ptr::null_mut(), &desc);
            if comp.is_null() {
                bail!("macOS has no voice-processing audio unit");
            }
            let mut unit: AudioUnit = std::ptr::null_mut();
            check(
                AudioComponentInstanceNew(comp, &mut unit),
                "opening the audio unit",
            )?;
            let one: u32 = 1;
            check(
                AudioUnitSetProperty(
                    unit,
                    kAudioOutputUnitProperty_EnableIO,
                    kAudioUnitScope_Input,
                    INPUT_BUS,
                    &one as *const u32 as *const c_void,
                    4,
                ),
                "turning the microphone on",
            )?;
            check(
                AudioUnitSetProperty(
                    unit,
                    kAudioOutputUnitProperty_EnableIO,
                    kAudioUnitScope_Output,
                    OUTPUT_BUS,
                    &one as *const u32 as *const c_void,
                    4,
                ),
                "turning the speakers on",
            )?;
            let bypass: u32 = if voice_processing { 0 } else { 1 };
            let _ = AudioUnitSetProperty(
                unit,
                kAUVoiceIOProperty_BypassVoiceProcessing,
                kAudioUnitScope_Global,
                INPUT_BUS,
                &bypass as *const u32 as *const c_void,
                4,
            );
            let fmt = AudioStreamBasicDescription {
                mSampleRate: crate::pcm::RATE as f64,
                mFormatID: kAudioFormatLinearPCM,
                mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
                mBytesPerPacket: 4,
                mFramesPerPacket: 1,
                mBytesPerFrame: 4,
                mChannelsPerFrame: 1,
                mBitsPerChannel: 32,
                mReserved: 0,
            };
            let size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
            check(
                AudioUnitSetProperty(
                    unit,
                    kAudioUnitProperty_StreamFormat,
                    kAudioUnitScope_Output,
                    INPUT_BUS,
                    &fmt as *const _ as *const c_void,
                    size,
                ),
                "setting the microphone's format",
            )?;
            check(
                AudioUnitSetProperty(
                    unit,
                    kAudioUnitProperty_StreamFormat,
                    kAudioUnitScope_Input,
                    OUTPUT_BUS,
                    &fmt as *const _ as *const c_void,
                    size,
                ),
                "setting the speakers' format",
            )?;
            let output: Output = Arc::new(Mutex::new(Default::default()));
            let ctx = Box::into_raw(Box::new(Ctx {
                unit,
                on_audio: Box::new(on_audio),
                output: output.clone(),
                buf: vec![0.0; 4096],
            }));
            let rec = AURenderCallbackStruct {
                inputProc: recorded,
                inputProcRefCon: ctx as *mut c_void,
            };
            check(
                AudioUnitSetProperty(
                    unit,
                    kAudioOutputUnitProperty_SetInputCallback,
                    kAudioUnitScope_Global,
                    INPUT_BUS,
                    &rec as *const _ as *const c_void,
                    std::mem::size_of::<AURenderCallbackStruct>() as u32,
                ),
                "listening to the microphone",
            )?;
            let ply = AURenderCallbackStruct {
                inputProc: play,
                inputProcRefCon: ctx as *mut c_void,
            };
            check(
                AudioUnitSetProperty(
                    unit,
                    kAudioUnitProperty_SetRenderCallback,
                    kAudioUnitScope_Input,
                    OUTPUT_BUS,
                    &ply as *const _ as *const c_void,
                    std::mem::size_of::<AURenderCallbackStruct>() as u32,
                ),
                "feeding the speakers",
            )?;
            check(AudioUnitInitialize(unit), "starting the audio unit")?;
            check(AudioOutputUnitStart(unit), "starting the microphone")?;
            Ok((Device { inner: unit }, output))
        }
    }
}
