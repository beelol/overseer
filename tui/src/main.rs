//! `overseer-tui`: watch and drive Overseer agents from a terminal, nine per page, live from the
//! same `overseerd` daemon VS Code uses.

use anyhow::Result;
use crossterm::event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use overseer_tui::app::App;
use overseer_tui::client::{Client, Msg};
use overseer_tui::locate::Daemon;
use overseer_tui::ui;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const HELP: &str = "overseer-tui — nine live Overseer agents per page, from the same overseerd daemon VS Code uses.

USAGE:
    overseer-tui [OPTIONS]

OPTIONS:
    --daemon PATH   overseerd binary (default: $OVERSEERD, next to this binary, $PATH, or the one
                    bundled with the Overseer VS Code extension)
    --home DIR      Overseer data directory (sets OVERSEER_HOME; default: the same as VS Code)
    --no-mouse      Leave the mouse to the terminal (text selection) instead of clicking tiles
    -h, --help      Show this help
    -V, --version   Show the version

KEYS:
    ←↓↑→ / h j k l  move between agents        1–9   focus agent n on this page
    tab / shift+tab next / previous agent      ] [   next / previous page (also PgDn/PgUp)
    i / enter       message the focused agent  z     zoom (full screen, scrollback)
    a / d           allow / deny a permission  w     next agent waiting for you
    x               interrupt                  n     new agent
    f               filter all/active/needs you ?     all keys
    q               quit (agents keep running)

Agents are pages of nine, newest first: page 1 is the newest nine.";

enum Ev {
    Daemon(Msg),
    Term(Event),
}

fn main() -> Result<()> {
    let mut daemon_path: Option<PathBuf> = None;
    let mut mouse = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("overseer-tui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--daemon" => daemon_path = args.next().map(PathBuf::from),
            "--home" => {
                if let Some(h) = args.next() {
                    std::env::set_var("OVERSEER_HOME", h);
                }
            }
            "--no-mouse" => mouse = false,
            other => anyhow::bail!("unknown option {other} (see --help)"),
        }
    }
    let daemon = Daemon::find(daemon_path.as_deref())?;
    let socket = daemon.socket_path()?;
    let (tx, rx) = mpsc::channel::<Ev>();
    let (dtx, drx) = mpsc::channel::<Msg>();
    let fwd = tx.clone();
    std::thread::spawn(move || {
        while let Ok(m) = drx.recv() {
            if fwd.send(Ev::Daemon(m)).is_err() {
                break;
            }
        }
    });
    let client = Arc::new(Client::start(Some(daemon), socket, dtx));
    let mut app = App::new(client.clone());
    app.cwd_repo = git_root();
    ui::set_truecolor(std::env::var("COLORTERM").map(|v| v.contains("truecolor") || v.contains("24bit")).unwrap_or(false));

    // ratatui::init sets raw mode and the alternate screen, and restores them on panic too.
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    if mouse {
        let _ = execute!(std::io::stdout(), EnableMouseCapture);
    }
    // Test-only: proves the terminal is restored after a panic (tests/look.rs).
    if std::env::var_os("OVERSEER_TUI_TEST_PANIC").is_some() {
        panic!("overseer-tui test panic");
    }
    let input = tx.clone();
    std::thread::spawn(move || {
        while let Ok(e) = event::read() {
            if input.send(Ev::Term(e)).is_err() {
                break;
            }
        }
    });

    let result = (|| -> Result<()> {
        let mut last_second = Instant::now();
        loop {
            if app.dirty {
                terminal.draw(|f| ui::draw(f, &mut app))?;
                app.dirty = false;
            }
            // Sleep until something happens; wake once a second only to refresh elapsed times.
            let timeout = if app.state.runs.iter().any(|r| r.active()) { Duration::from_millis(1000) } else { Duration::from_secs(30) };
            let first = rx.recv_timeout(timeout.min(Duration::from_millis(100)));
            let mut batch: Vec<Ev> = first.into_iter().collect();
            while let Ok(e) = rx.try_recv() {
                batch.push(e);
            }
            for ev in batch {
                match ev {
                    Ev::Daemon(m) => app.handle_msg(m),
                    Ev::Term(Event::Key(k)) if k.kind != KeyEventKind::Release => app.handle_key(k),
                    Ev::Term(Event::Mouse(m)) => {
                        app.handle_mouse(m);
                        app.dirty = true;
                    }
                    Ev::Term(Event::Paste(t)) => app.paste(&t),
                    Ev::Term(Event::Resize(..)) => app.dirty = true,
                    Ev::Term(_) => {}
                }
            }
            let now = Instant::now();
            if app.tick(now) {
                app.dirty = true;
            }
            if now.duration_since(last_second) >= timeout {
                last_second = now;
                if app.state.runs.iter().any(|r| r.active()) {
                    app.dirty = true;
                }
            }
            if app.quit {
                return Ok(());
            }
        }
    })();
    if mouse {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    client.close();
    result
}

fn git_root() -> Option<String> {
    let out = std::process::Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
