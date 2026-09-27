//! The listener's loop: audio in, in 20 ms frames, through the speech gate and the endpointer;
//! words out; Overseer's voice played back in step with the input.
//!
//! Everything runs on the stream's own clock (samples heard so far), so a test can feed a file as
//! fast as the machine allows and get the same events as in real time. Recognition and speech
//! synthesis run inline when pacing is `Fast` (deterministic) and on worker threads otherwise, so a
//! slow recognizer never holds up the microphone.

use crate::endpoint::{EndAction, EndConfig, Endpointer};
use crate::gate::{GateConfig, GateEvent, SpeechGate};
use crate::pcm::{self, FRAME, RATE};
use crate::protocol::{Command, Event, Source};
use crate::recognize::{Recognizer, VOCABULARY};
use crate::speak::{self, SpeakEvent, Speaker};
use crate::words;
use anyhow::Result;
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;

#[derive(Clone, Debug)]
pub enum Input {
    /// The microphone, through macOS's voice-processing unit (echo cancellation) or plainly.
    Mic {
        voice_processing: bool,
    },
    File(PathBuf),
    /// 16-bit little-endian mono PCM at 16 kHz on standard input.
    Stdin,
    /// Audio in `feed` commands (the daemon's simulated voice).
    Feed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pace {
    Realtime,
    Fast,
}

pub struct Options {
    pub input: Input,
    pub pace: Pace,
    pub gate: GateConfig,
    pub end: EndConfig,
    /// Words to expect besides the vocabulary: agent names, repositories.
    pub hint: String,
    pub voice: Option<String>,
    pub rate: Option<u32>,
    /// Tests: Overseer's voice mixed back into the input at this gain, 40 ms late, as a room with
    /// no echo cancellation would.
    pub echo: f32,
    /// Tests: commands applied when the stream reaches `at_ms`.
    pub timed: Vec<(u64, Command)>,
    /// Whether commands come on standard input.
    pub control: bool,
    /// A level for the mark at most every this many frames (2 frames: 25 a second).
    pub level_every: usize,
    /// Tests: Overseer's voice from this function instead of `say`.
    pub synth: Option<fn(&str) -> Vec<f32>>,
    /// The live script's lines, when the recognizer is `Scripted` with a live part.
    pub script: Option<std::sync::Arc<std::sync::Mutex<Vec<crate::recognize::ScriptLine>>>>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            input: Input::Feed,
            pace: Pace::Realtime,
            gate: GateConfig::default(),
            end: EndConfig::default(),
            hint: String::new(),
            voice: None,
            rate: None,
            echo: 0.0,
            timed: Vec::new(),
            control: true,
            level_every: 2,
            synth: None,
            script: None,
        }
    }
}

enum Msg {
    Frame(Vec<f32>),
    InputEnded,
    Cmd(Command),
    Heard {
        job: Job,
        text: String,
    },
    Spoken {
        line: u64,
        phrase: usize,
        text: String,
        audio: Vec<f32>,
    },
    Fail(String),
}

#[derive(Clone, Debug)]
enum Job {
    /// Words so far, while the owner speaks.
    Partial { id: u64 },
    /// Words so far, when the silence reached the check point.
    Check { id: u64 },
    /// All the words, at the end of the thought.
    Final { id: u64, start_ms: u64, end_ms: u64 },
}

impl Job {
    fn id(&self) -> u64 {
        match self {
            Job::Partial { id } | Job::Check { id } | Job::Final { id, .. } => *id,
        }
    }
}

/// Sends events as JSON lines.
pub struct Out<W: Write> {
    w: W,
}

impl<W: Write> Out<W> {
    pub fn new(w: W) -> Self {
        Self { w }
    }
    pub fn send(&mut self, e: &Event) {
        if let Ok(line) = serde_json::to_string(e) {
            let _ = writeln!(self.w, "{line}");
            let _ = self.w.flush();
        }
    }
}

struct State<W: Write> {
    opts_hint: String,
    out: Out<W>,
    gate: SpeechGate,
    end: Endpointer,
    speaker: Speaker,
    /// Samples heard so far: the stream's clock.
    t: u64,
    frames: u64,
    suppress_until: u64,
    echo: f32,
    echo_line: VecDeque<f32>,
    /// While a recognition runs on the worker, no new partial is asked for.
    busy: bool,
    /// The utterance length (samples) at which the next partial is due.
    next_partial: usize,
    /// The utterance whose words lowered Overseer's voice, if any.
    barged: Option<u64>,
    level_owner: f32,
    level_overseer: f32,
    level_every: usize,
    pending_timed: VecDeque<(u64, Command)>,
    recognizer: Option<Box<dyn Recognizer>>,
    work: Option<mpsc::Sender<(Job, Vec<f32>, u64, String)>>,
    synth_work: Option<mpsc::Sender<(u64, usize, String)>>,
    synth: Box<dyn FnMut(&str) -> Result<Vec<f32>> + Send>,
    starts: std::collections::HashMap<u64, u64>,
    output: Option<std::sync::Arc<std::sync::Mutex<VecDeque<f32>>>>,
    /// Phrases already made (the common lines), by text.
    made: std::collections::HashMap<String, Vec<f32>>,
    script: Option<std::sync::Arc<std::sync::Mutex<Vec<crate::recognize::ScriptLine>>>>,
}

fn ms(samples: u64) -> u64 {
    samples * 1000 / RATE as u64
}

impl<W: Write> State<W> {
    fn now_ms(&self) -> u64 {
        ms(self.t)
    }

    fn hint(&self) -> String {
        format!("{VOCABULARY} {}", self.opts_hint)
            .trim()
            .to_string()
    }

    fn ask(&mut self, job: Job, audio: Vec<f32>, start_ms: u64, tx: &mpsc::Sender<Msg>) {
        let hint = self.hint();
        if let Some(r) = self.recognizer.as_mut() {
            let text = match r.words(&audio, start_ms, &hint) {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.send(Msg::Fail(format!("the recognizer failed: {e}")));
                    String::new()
                }
            };
            self.heard(job, text);
        } else if let Some(w) = &self.work {
            self.busy = true;
            let _ = w.send((job, audio, start_ms, hint));
        } else if let Job::Final {
            id,
            start_ms,
            end_ms,
        } = job
        {
            // No recognizer at all (levels only): an utterance with no words.
            self.out.send(&Event::Dropped {
                id,
                text: String::new(),
                reason: format!("no recognizer ({start_ms}-{end_ms} ms)"),
                t_ms: self.now_ms(),
            });
        }
    }

    fn heard(&mut self, job: Job, text: String) {
        let t_ms = self.now_ms();
        let echo = words::is_echo(&text, &self.speaker.recent_words());
        match job {
            Job::Partial { id } | Job::Check { id } => {
                self.end.set_words(id, &text);
                if text.is_empty() || echo {
                    return;
                }
                self.out.send(&Event::Words {
                    id,
                    text: text.clone(),
                    t_ms,
                });
                self.barge(id, &text);
            }
            Job::Final {
                id,
                start_ms,
                end_ms,
            } => {
                if self.barged == Some(id) {
                    // The utterance that lowered the voice has ended and nothing decided otherwise.
                    self.barged = None;
                    if let Some(e) = self.speaker.restore() {
                        self.spoke(e);
                    }
                }
                let reason = if text.is_empty() {
                    Some("empty")
                } else if echo {
                    Some("echo")
                } else {
                    None
                };
                match reason {
                    Some(r) => self.out.send(&Event::Dropped {
                        id,
                        text,
                        reason: r.into(),
                        t_ms,
                    }),
                    None => {
                        let complete = !crate::endpoint::reads_unfinished(&text);
                        self.out.send(&Event::Utterance {
                            id,
                            text,
                            complete,
                            start_ms,
                            end_ms,
                            t_ms,
                        });
                    }
                }
            }
        }
    }

    /// Two or more words over Overseer's voice lower it; a stop word stops it (AC-164).
    fn barge(&mut self, id: u64, text: &str) {
        if !self.speaker.speaking() {
            return;
        }
        if words::leads_with_stop_word(text) {
            for e in self.speaker.stop_now(None) {
                self.spoke(e);
            }
            self.barged = None;
            self.out.send(&Event::Barge {
                id,
                text: text.to_string(),
                stop_word: true,
                t_ms: self.now_ms(),
            });
            return;
        }
        if self.barged.is_none()
            && words::content_words(text).len() >= 2
            && !words::is_backchannel(text)
        {
            if let Some(e) = self.speaker.lower() {
                self.barged = Some(id);
                self.spoke(e);
                self.out.send(&Event::Barge {
                    id,
                    text: text.to_string(),
                    stop_word: false,
                    t_ms: self.now_ms(),
                });
            }
        }
    }

    fn spoke(&mut self, e: SpeakEvent) {
        let t_ms = self.now_ms();
        let (line, event, phrase) = match e {
            SpeakEvent::Start { line } => (line, "start", 0),
            SpeakEvent::Lowered { line, phrase } => (line, "lowered", phrase),
            SpeakEvent::Restored { line, phrase } => (line, "restored", phrase),
            SpeakEvent::Stopped { line, phrase } => (line, "stopped", phrase),
            SpeakEvent::Phrase { line, phrase } => (line, "phrase", phrase),
            SpeakEvent::Done { line } => (line, "done", 0),
        };
        if matches!(event, "stopped" | "done") {
            self.barged = None;
        }
        self.out.send(&Event::Spoke {
            line,
            event: event.into(),
            phrase,
            t_ms,
        });
    }

    fn command(&mut self, c: Command, tx: &mpsc::Sender<Msg>) -> bool {
        match c {
            Command::Speak { line, text } => {
                for (l, p, phrase) in self.speaker.queue(line, &text) {
                    if let Some(a) = self.made.get(&phrase) {
                        self.speaker.synthesized(l, p, a.clone());
                    } else if let Some(w) = &self.synth_work {
                        let _ = w.send((l, p, phrase));
                    } else {
                        match (self.synth)(&phrase) {
                            Ok(a) => self.speaker.synthesized(l, p, a),
                            Err(e) => self.out.send(&Event::Error {
                                message: format!("could not speak: {e}"),
                                t_ms: self.now_ms(),
                            }),
                        }
                    }
                }
            }
            Command::Restore => {
                self.barged = None;
                if let Some(e) = self.speaker.restore() {
                    self.spoke(e);
                }
            }
            Command::Stop { line, at } => {
                if at.as_deref() == Some("phrase") {
                    self.speaker.stop_at_phrase();
                } else {
                    if let Some(o) = &self.output {
                        o.lock().unwrap().clear();
                    }
                    for e in self.speaker.stop_now(line) {
                        self.spoke(e);
                    }
                }
            }
            Command::Hint { text } => self.opts_hint = text,
            Command::Suppress { ms } => {
                self.suppress_until = self.suppress_until.max(self.t + ms * RATE as u64 / 1000)
            }
            Command::Feed { pcm } => {
                use base64::Engine;
                match base64::engine::general_purpose::STANDARD.decode(pcm) {
                    Ok(bytes) => {
                        let _ = tx.send(Msg::Frame(pcm::from_s16le(&bytes)));
                    }
                    Err(e) => self.out.send(&Event::Error {
                        message: format!("bad feed: {e}"),
                        t_ms: self.now_ms(),
                    }),
                }
            }
            Command::FeedEnd => {
                let _ = tx.send(Msg::InputEnded);
            }
            Command::Script { lines } => match &self.script {
                Some(s) => s.lock().unwrap().extend(lines),
                None => self.out.send(&Event::Error {
                    message: "this listener was not started with --script-live".into(),
                    t_ms: self.now_ms(),
                }),
            },
            Command::Quit => return false,
        }
        true
    }

    /// One 20 ms frame of input.
    fn frame(&mut self, mut frame: Vec<f32>, tx: &mpsc::Sender<Msg>) {
        // Commands scheduled for this moment (tests).
        while self
            .pending_timed
            .front()
            .is_some_and(|(at, _)| *at <= self.now_ms())
        {
            let (_, c) = self.pending_timed.pop_front().unwrap();
            self.command(c, tx);
        }
        // Overseer's voice for the same 20 ms.
        let (voice, events, level) = self.speaker.next(FRAME);
        for e in events {
            self.spoke(e);
        }
        if let Some(o) = &self.output {
            o.lock().unwrap().extend(voice.iter().copied());
        }
        if self.echo > 0.0 {
            // A room with no echo cancellation: Overseer's voice comes back 40 ms later.
            self.echo_line.extend(voice.iter().map(|v| v * self.echo));
            for s in frame.iter_mut() {
                *s += self.echo_line.pop_front().unwrap_or(0.0);
            }
        }
        self.t += FRAME as u64;
        self.frames += 1;
        let suppressed = self.t <= self.suppress_until;
        let (gate_event, open) = if suppressed {
            (None, false)
        } else {
            let (_, ev) = self.gate.push(&frame);
            (ev, self.gate.is_open())
        };
        match gate_event {
            Some(GateEvent::Open { .. }) => self.out.send(&Event::Gate {
                open: true,
                t_ms: self.now_ms(),
            }),
            Some(GateEvent::Close) => self.out.send(&Event::Gate {
                open: false,
                t_ms: self.now_ms(),
            }),
            None => {}
        }
        // Levels for the mark: the owner's only while the gate is open; Overseer's while it speaks.
        self.level_owner = self
            .level_owner
            .max(if open { self.gate.level() } else { 0.0 });
        if let Some(l) = level {
            self.level_overseer = self.level_overseer.max(l);
        }
        if self.frames % self.level_every as u64 == 0 {
            if open {
                self.out.send(&Event::Level {
                    source: Source::Owner,
                    value: round(self.level_owner),
                    t_ms: self.now_ms(),
                });
            }
            if level.is_some() {
                self.out.send(&Event::Level {
                    source: Source::Overseer,
                    value: round(self.level_overseer),
                    t_ms: self.now_ms(),
                });
            }
            self.level_owner = 0.0;
            self.level_overseer = 0.0;
        }
        for action in self.end.push(&frame, gate_event, open, self.t) {
            match action {
                EndAction::Started { id, start_ms } => {
                    self.starts.insert(id, start_ms);
                    // Words are wanted sooner while Overseer speaks: they decide whether it yields.
                    let first = if self.speaker.speaking() {
                        RATE as usize * 2 / 5
                    } else {
                        RATE as usize
                    };
                    self.next_partial = self.end.audio().len() + first;
                }
                EndAction::NeedWords { id } => {
                    let audio = self.end.audio().to_vec();
                    let start = self.starts.get(&id).copied().unwrap_or(0);
                    if !self.busy {
                        self.ask(Job::Check { id }, audio, start, tx);
                    }
                }
                EndAction::Ended {
                    id,
                    start_ms,
                    end_ms,
                    audio,
                    ..
                } => {
                    self.starts.remove(&id);
                    self.ask(
                        Job::Final {
                            id,
                            start_ms,
                            end_ms,
                        },
                        audio,
                        start_ms,
                        tx,
                    );
                }
            }
        }
        // Partial words: every second of speech; every 0.3 s while Overseer speaks, also in the
        // pauses, so a short "stop" is caught as soon as it is said.
        if let Some(id) = self.end.current() {
            let len = self.end.audio().len();
            let speaking = self.speaker.speaking();
            if (open || speaking) && len >= self.next_partial && !self.busy {
                self.next_partial = len
                    + if speaking {
                        RATE as usize * 3 / 10
                    } else {
                        RATE as usize
                    };
                let audio = self.end.audio().to_vec();
                let start = self.starts.get(&id).copied().unwrap_or(0);
                self.ask(Job::Partial { id }, audio, start, tx);
            }
        }
    }
}

fn round(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

/// Runs the listener until the input ends (files, standard input, a finished feed) or `quit`.
pub fn run<W: Write + Send + 'static>(
    opts: Options,
    recognizer: Option<Box<dyn Recognizer>>,
    out: W,
) -> Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let inline = opts.pace == Pace::Fast;
    let model = recognizer
        .as_ref()
        .map(|r| r.name())
        .unwrap_or_else(|| "none".into());
    let voice = opts.voice.clone();
    let rate = opts.rate;
    let synth_fn = opts.synth;
    let synth: Box<dyn FnMut(&str) -> Result<Vec<f32>> + Send> = match synth_fn {
        Some(f) => Box::new(move |t: &str| Ok(f(t))),
        None => Box::new(move |t: &str| speak::synthesize(t, voice.as_deref(), rate)),
    };
    let mut st = State {
        opts_hint: opts.hint.clone(),
        out: Out::new(out),
        gate: SpeechGate::new(opts.gate.clone()),
        end: Endpointer::new(opts.end.clone()),
        speaker: Speaker::new(),
        t: 0,
        frames: 0,
        suppress_until: 0,
        echo: opts.echo,
        echo_line: std::iter::repeat_n(0.0, RATE as usize * 40 / 1000).collect(),
        busy: false,
        next_partial: RATE as usize,
        barged: None,
        level_owner: 0.0,
        level_overseer: 0.0,
        level_every: opts.level_every.max(1),
        pending_timed: opts.timed.iter().cloned().collect(),
        recognizer: None,
        work: None,
        synth_work: None,
        synth,
        starts: Default::default(),
        output: None,
        made: Default::default(),
        script: opts.script.clone(),
    };
    // Recognition: inline for a deterministic fast run, otherwise on its own thread.
    if let Some(r) = recognizer {
        if inline {
            st.recognizer = Some(r);
        } else {
            let (wtx, wrx) = mpsc::channel::<(Job, Vec<f32>, u64, String)>();
            let back = tx.clone();
            let mut r = r;
            std::thread::spawn(move || {
                while let Ok((job, audio, start, hint)) = wrx.recv() {
                    match r.words(&audio, start, &hint) {
                        Ok(text) => {
                            let _ = back.send(Msg::Heard { job, text });
                        }
                        Err(e) => {
                            let _ = back.send(Msg::Fail(format!("the recognizer failed: {e}")));
                            let _ = back.send(Msg::Heard {
                                job,
                                text: String::new(),
                            });
                        }
                    }
                }
            });
            st.work = Some(wtx);
        }
    }
    if !inline {
        let (stx, srx) = mpsc::channel::<(u64, usize, String)>();
        let back = tx.clone();
        let voice = opts.voice.clone();
        std::thread::spawn(move || {
            while let Ok((line, phrase, text)) = srx.recv() {
                let audio = match synth_fn {
                    Some(f) => Ok(f(&text)),
                    None => speak::synthesize(&text, voice.as_deref(), rate),
                };
                match audio {
                    Ok(audio) => {
                        let _ = back.send(Msg::Spoken {
                            line,
                            phrase,
                            text,
                            audio,
                        });
                    }
                    Err(e) => {
                        let _ = back.send(Msg::Fail(format!("could not speak: {e}")));
                    }
                }
            }
        });
        // The common lines, made ahead (line 0 means "keep, do not play").
        for (i, text) in speak::COMMON.iter().enumerate() {
            let _ = stx.send((0, i, text.to_string()));
        }
        st.synth_work = Some(stx);
    }
    // Commands on standard input.
    if opts.control && !matches!(opts.input, Input::Stdin) {
        let back = tx.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                match std::io::BufRead::read_line(&mut stdin.lock(), &mut line) {
                    Ok(0) | Err(_) => {
                        let _ = back.send(Msg::Cmd(Command::Quit));
                        break;
                    }
                    Ok(_) => match serde_json::from_str::<Command>(line.trim()) {
                        Ok(c) => {
                            if back.send(Msg::Cmd(c)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = back.send(Msg::Fail(format!("bad command: {e}")));
                        }
                    },
                }
            }
        });
    }
    // The input.
    let input_name = match &opts.input {
        Input::Mic { voice_processing } => {
            let back = tx.clone();
            let (device, output) = crate::mic::open(*voice_processing, move |s| {
                let _ = back.send(Msg::Frame(s));
            })?;
            st.output = Some(output);
            std::mem::forget(device); // the device lives as long as the process
            if *voice_processing {
                "mic (voice processing)"
            } else {
                "mic"
            }
            .to_string()
        }
        Input::File(path) => {
            let samples = pcm::read_wav(&std::fs::read(path)?)?;
            let back = tx.clone();
            let pace = opts.pace;
            std::thread::spawn(move || feed_samples(samples, pace, back));
            format!("file {}", path.display())
        }
        Input::Stdin => {
            let back = tx.clone();
            let pace = opts.pace;
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes);
                feed_samples(pcm::from_s16le(&bytes), pace, back);
            });
            "stdin".to_string()
        }
        Input::Feed => "feed".to_string(),
    };
    st.out.send(&Event::Ready {
        input: input_name,
        model,
        t_ms: 0,
    });
    let mut carry: Vec<f32> = Vec::new();
    let mut ended = false;
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Frame(samples) => {
                carry.extend(samples);
                while carry.len() >= FRAME {
                    let f: Vec<f32> = carry.drain(..FRAME).collect();
                    st.frame(f, &tx);
                }
            }
            Msg::InputEnded => {
                if ended {
                    continue;
                }
                ended = true;
                // Let a last utterance end as it would in silence, then say the input ended.
                for _ in 0..(st_end_frames(&opts.end)) {
                    st.frame(vec![0.0; FRAME], &tx);
                }
                // Wait for words still on the worker.
                while st.busy {
                    match rx.recv() {
                        Ok(Msg::Heard { job, text }) => {
                            st.busy = false;
                            st.heard(job, text);
                        }
                        Ok(Msg::Fail(m)) => st.out.send(&Event::Error {
                            message: m,
                            t_ms: st.now_ms(),
                        }),
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                let t_ms = st.now_ms();
                st.out.send(&Event::End { t_ms });
                if !opts.control || matches!(opts.input, Input::File(_) | Input::Stdin) {
                    break;
                }
            }
            Msg::Cmd(c) => {
                if !st.command(c, &tx) {
                    break;
                }
            }
            Msg::Heard { job, text } => {
                st.busy = false;
                let _ = job.id();
                st.heard(job, text);
            }
            Msg::Spoken {
                line: 0,
                text,
                audio,
                ..
            } => {
                st.made.insert(text, audio);
            }
            Msg::Spoken {
                line,
                phrase,
                text,
                audio,
            } => {
                if speak::COMMON.contains(&text.as_str()) {
                    st.made.entry(text).or_insert_with(|| audio.clone());
                }
                st.speaker.synthesized(line, phrase, audio);
            }
            Msg::Fail(m) => {
                let t_ms = st.now_ms();
                st.out.send(&Event::Error { message: m, t_ms });
            }
        }
    }
    Ok(())
}

fn st_end_frames(end: &EndConfig) -> u64 {
    (end.unfinished_ms + 500) / 20
}

/// Sends samples as frames, at real-time pace or as fast as they are taken.
fn feed_samples(samples: Vec<f32>, pace: Pace, tx: mpsc::Sender<Msg>) {
    let start = std::time::Instant::now();
    for (i, f) in samples.chunks(FRAME).enumerate() {
        if pace == Pace::Realtime {
            let due = start + std::time::Duration::from_millis(20 * i as u64);
            if let Some(wait) = due.checked_duration_since(std::time::Instant::now()) {
                std::thread::sleep(wait);
            }
        }
        if tx.send(Msg::Frame(f.to_vec())).is_err() {
            return;
        }
    }
    let _ = tx.send(Msg::InputEnded);
}
