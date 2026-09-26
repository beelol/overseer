//! Test support: a real `overseerd` in an isolated `OVERSEER_HOME`, fixture repositories, and a
//! driver that runs the TUI's app loop against the daemon and renders it into an off-screen
//! terminal buffer (the same `ui::draw` the binary uses).
#![allow(dead_code)]

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use overseer_tui::app::App;
use overseer_tui::client::{Client, Msg, Requests};
use overseer_tui::ui;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

static BUILT: OnceLock<PathBuf> = OnceLock::new();

/// The workspace's `overseerd` (built once per test run).
pub fn daemon_binary() -> PathBuf {
    BUILT
        .get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
            let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "overseerd"]).current_dir(&root).stderr(Stdio::null()).status().expect("cargo build overseerd");
            assert!(status.success(), "building overseerd failed");
            let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
            target.join("debug").join("overseerd")
        })
        .clone()
}

pub struct Daemon {
    pub home: tempfile::TempDir,
    pub bin: PathBuf,
    pub socket: PathBuf,
    child: Child,
    env: Vec<(String, String)>,
}

impl Daemon {
    pub fn start(env: &[(&str, &str)]) -> Daemon {
        let home = tempfile::tempdir().unwrap();
        let bin = daemon_binary();
        let env: Vec<(String, String)> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut cmd = Command::new(&bin);
        cmd.arg("serve").env("OVERSEER_HOME", home.path()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        for (k, v) in &env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().unwrap();
        let out = Command::new(&bin).arg("socket-path").env("OVERSEER_HOME", home.path()).output().unwrap();
        let socket = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        for _ in 0..100 {
            if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Daemon { home, bin, socket, child, env }
    }

    /// Whether the daemon process started by this test has exited.
    pub fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// `overseerd ctl` (a separate client, like VS Code or a script).
    pub fn ctl(&self, method: &str, params: Value) -> Value {
        let out = Command::new(&self.bin).args(["ctl", method, &params.to_string()]).env("OVERSEER_HOME", self.home.path()).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let v: Value = serde_json::from_str(text.lines().next().unwrap_or("{}")).unwrap_or(json!({}));
        if !v["error"].is_null() {
            panic!("{method} failed: {}", v["error"]);
        }
        v["result"].clone()
    }

    pub fn run(&self, id: &str) -> Value {
        self.ctl("state", json!({}))["runs"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap_or(Value::Null)
    }

    pub fn events(&self, run: &str) -> Vec<Value> {
        self.ctl("events.list", json!({ "run_id": run, "after": 0, "limit": 5000 }))["events"].as_array().cloned().unwrap_or_default()
    }

    /// A generic agent in `repo` running `sh -c script` (no paid tokens).
    pub fn sh(&self, repo: &Path, title: &str, script: &str) -> String {
        let t = self.ctl("task.create", json!({ "repo": repo, "harness": "generic", "program": "/bin/sh", "args": ["-c", script], "prompt": "", "title": title, "workspace_mode": "worktree" }));
        t["run"]["id"].as_str().unwrap().to_string()
    }

    pub fn wait_status(&self, run: &str, want: impl Fn(&str) -> bool, secs: u64) -> String {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let st = self.run(run)["status"].as_str().unwrap_or_default().to_string();
            if want(&st) || Instant::now() > end {
                return st;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = Command::new(&self.bin).args(["ctl", "daemon.stop_all", "{}"]).env("OVERSEER_HOME", self.home.path()).output();
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn repo(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let git = |args: &[&str]| assert!(Command::new("git").args(args).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "git {args:?}");
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Overseer Test"]);
    git(&["config", "user.email", "overseer-test@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("README.md"), "# fixture\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "base"]);
    dir.canonicalize().unwrap()
}

/// The TUI app driven off-screen against a real daemon.
pub struct Tui {
    pub app: App,
    pub client: Arc<Client>,
    rx: Receiver<Msg>,
    pub term: Terminal<TestBackend>,
}

impl Tui {
    pub fn attach(d: &Daemon, w: u16, h: u16) -> Tui {
        Tui::attach_with(d, w, h, false)
    }

    /// Like `attach`, but the TUI may start the daemon itself (always in the test's home).
    pub fn attach_spawning(d: &Daemon, w: u16, h: u16) -> Tui {
        Tui::attach_with(d, w, h, true)
    }

    fn attach_with(d: &Daemon, w: u16, h: u16, spawn: bool) -> Tui {
        let (tx, rx) = channel();
        let daemon = spawn.then(|| overseer_tui::locate::Daemon { binary: d.bin.clone(), home: Some(d.home.path().to_path_buf()) });
        let jobs = tx.clone();
        let client = Arc::new(Client::start(daemon, d.socket.clone(), tx));
        let mut app = App::new(client.clone() as Arc<dyn Requests>);
        app.set_jobs(jobs);
        let mut t = Tui { app, client, rx, term: Terminal::new(TestBackend::new(w, h)).unwrap() };
        t.until(10, |a| a.connected);
        t.pump(400);
        t
    }

    /// Processes daemon messages and timers for `ms` milliseconds.
    pub fn pump(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.step(Duration::from_millis(10));
        }
    }

    fn step(&mut self, wait: Duration) {
        if let Ok(m) = self.rx.recv_timeout(wait) {
            self.app.handle_msg(m);
            while let Ok(m) = self.rx.try_recv() {
                self.app.handle_msg(m);
            }
        }
        self.app.tick(Instant::now());
    }

    /// Pumps until `cond` holds (panics after `secs`).
    pub fn until(&mut self, secs: u64, cond: impl Fn(&App) -> bool) {
        let end = Instant::now() + Duration::from_secs(secs);
        while !cond(&self.app) {
            assert!(Instant::now() < end, "timed out; screen:\n{}", self.screen());
            self.step(Duration::from_millis(10));
        }
    }

    /// Pumps until the rendered screen contains `text`.
    pub fn until_screen(&mut self, secs: u64, text: &str) -> String {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let s = self.screen();
            if s.contains(text) {
                return s;
            }
            assert!(Instant::now() < end, "timed out waiting for {text:?}; screen:\n{s}");
            self.step(Duration::from_millis(20));
        }
    }

    pub fn key(&mut self, code: KeyCode) {
        self.key_mod(code, KeyModifiers::NONE);
    }

    pub fn key_mod(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.app.handle_key(KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE });
        self.pump(30);
    }

    pub fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            self.app.handle_key(KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE });
        }
        self.pump(30);
    }

    pub fn draw(&mut self) -> Buffer {
        self.term.draw(|f| ui::draw(f, &mut self.app)).unwrap();
        self.term.backend().buffer().clone()
    }

    /// The screen as text (one line per row).
    pub fn screen(&mut self) -> String {
        let buf = self.draw();
        buffer_text(&buf)
    }

    /// Saves the current screen as evidence (text and SVG).
    pub fn snapshot(&mut self, name: &str) {
        let buf = self.draw();
        snapshot(&buf, name);
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.term.backend_mut().resize(w, h);
        self.app.dirty = true;
    }
}

pub fn buffer_text(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut skip = 0;
        for x in 0..buf.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let sym = buf[(x, y)].symbol();
            out.push_str(sym);
            skip = unicode_width::UnicodeWidthStr::width(sym).saturating_sub(1);
        }
        out.push('\n');
    }
    out
}

/// Writes a snapshot of the screen as text and as a colored SVG under
/// docs/verification/evidence/tui/ (evidence for the pull request).
pub fn snapshot(buf: &Buffer, name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("docs/verification/evidence/tui");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.txt")), buffer_text(buf)).unwrap();
    std::fs::write(dir.join(format!("{name}.svg")), overseer_tui_svg(buf, false)).unwrap();
    // The same screen in a light terminal (terminal default colors flip; accents stay).
    std::fs::write(dir.join(format!("{name}-light.svg")), overseer_tui_svg(buf, true)).unwrap();
}

fn overseer_tui_svg(buf: &Buffer, light: bool) -> String {
    use ratatui::style::{Color, Modifier};
    let (cw, ch) = (8.4_f32, 17.0_f32);
    let w = buf.area.width as f32 * cw + 24.0;
    let h = buf.area.height as f32 * ch + 24.0;
    // Typical terminal palettes (dark: a graphite theme; light: a paper theme).
    let fg_default = if light { "#1c1a26" } else { "#e6e4ef" };
    let bg = if light { "#fbfaf7" } else { "#15141b" };
    let color = |c: Color, default: &str| -> String {
        match (c, light) {
            (Color::Reset, _) => default.to_string(),
            (Color::Black, _) => "#1c1b24".into(),
            (Color::Red, false) => "#e5677a".into(),
            (Color::Red, true) => "#c4314b".into(),
            (Color::Green, false) => "#58c48d".into(),
            (Color::Green, true) => "#1f8a55".into(),
            (Color::Yellow, false) => "#e8c35a".into(),
            (Color::Yellow, true) => "#9a6a00".into(),
            (Color::Blue, _) => "#3b73d9".into(),
            (Color::Magenta, _) => "#a64fd8".into(),
            (Color::Cyan, false) => "#5fd0e0".into(),
            (Color::Cyan, true) => "#0b7f93".into(),
            (Color::Gray, _) => "#8d89a0".into(),
            (Color::DarkGray, false) => "#7d7990".into(),
            (Color::DarkGray, true) => "#6b6780".into(),
            (Color::White, _) => "#ffffff".into(),
            (Color::Indexed(141), false) => "#9b7bff".into(),
            (Color::Indexed(141), true) => "#7c5ce0".into(),
            (Color::Indexed(214), false) => "#f2a83b".into(),
            (Color::Indexed(214), true) => "#b86a00".into(),
            (Color::Rgb(r, g, b), _) => format!("#{r:02x}{g:02x}{b:02x}"),
            _ => default.to_string(),
        }
    };
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><rect width=\"100%\" height=\"100%\" rx=\"10\" fill=\"{bg}\"/><g font-family=\"ui-monospace, SFMono-Regular, Menlo, monospace\" font-size=\"14\">");
    for y in 0..buf.area.height {
        let mut x = 0;
        while x < buf.area.width {
            let cell = &buf[(x, y)];
            let fg = color(cell.fg, fg_default);
            let bold = cell.modifier.contains(Modifier::BOLD);
            let italic = cell.modifier.contains(Modifier::ITALIC);
            // Group cells of the same style into one text run.
            let mut text = String::new();
            let start = x;
            let mut cells = 0u16;
            while x < buf.area.width {
                let c = &buf[(x, y)];
                if color(c.fg, fg_default) != fg || c.modifier.contains(Modifier::BOLD) != bold || c.modifier.contains(Modifier::ITALIC) != italic {
                    break;
                }
                text.push_str(c.symbol());
                let w = unicode_width::UnicodeWidthStr::width(c.symbol()).max(1) as u16;
                x += w;
                cells += w;
            }
            if text.trim().is_empty() {
                continue;
            }
            // Pin the run to its cells so any monospace font lines up with the grid.
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" textLength=\"{:.1}\" lengthAdjust=\"spacingAndGlyphs\" fill=\"{fg}\"{}{} xml:space=\"preserve\">{}</text>",
                12.0 + start as f32 * cw,
                12.0 + (y as f32 + 0.78) * ch,
                cells as f32 * cw,
                if bold { " font-weight=\"700\"" } else { "" },
                if italic { " font-style=\"italic\"" } else { "" },
                esc(&text)
            ));
        }
    }
    out.push_str("</g></svg>\n");
    out
}
