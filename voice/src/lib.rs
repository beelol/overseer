//! Overseer's voice listener (Voice Mode, Gate R). It runs as its own process, started by the
//! daemon: it collects audio on the Mac, finds speech (the speech gate), turns it into words and
//! speaks Overseer's answers. It sends the daemon words and one loudness level, never the recording.

pub mod calls;
pub mod endpoint;
pub mod gate;
pub mod listener;
pub mod memspeech;
pub mod mic;
pub mod pcm;
pub mod protocol;
pub mod recognize;
pub mod speak;
pub mod synth;
pub mod words;
