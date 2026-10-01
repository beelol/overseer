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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const HELP: &str = "overseer-tui — up to sixteen live Overseer agents per page, from the same overseerd daemon VS Code uses.

USAGE:
    overseer-tui [OPTIONS]

OPTIONS:
    --daemon PATH   overseerd binary (default: $OVERSEERD, next to this binary, $PATH, or the one
                    bundled with the Overseer VS Code extension)
    --home DIR      Overseer data directory (default: the same as VS Code; the installed TUI
                    ignores OVERSEER_HOME and OVERSEER_SOCKET in its environment)
    --no-mouse      Leave the mouse to the terminal (text selection) instead of clicking tiles
    --no-bell       No terminal bell when an agent starts waiting for you
    --dashboard     Start in dashboard mode: the agent list, the picked agent's review and its
                    conversation side by side (needs 160 columns; D switches to the grid)
    --grid          Show only the grid of agents (no list, no conversation column), e.g. in a
                    second terminal beside one in dashboard mode
    -h, --help      Show this help
    -V, --version   Show the version

KEYS:
    ←↓↑→ / h j k l  move between agents        1–9   focus agent n on this page
    tab / shift+tab next / previous agent      ] [   next / previous page (also PgDn/PgUp)
    J / K           pick in the agent list (its conversation beside the grid; esc closes it)
    L               hide or show the agent list
    D               dashboard mode <-> the grid, on the picked agent (tab: list, review,
                    conversation; J / K pick another agent from any column)
    i / enter       message the focused agent  g / z grid <-> the focused agent's full view
    a / s / d       allow / deny a permission: a once, s for this session, d with a note
    w               next agent waiting for you
    x               interrupt                  n     new agent
    C               remove a finished agent's worktree (branch kept)
    P               open a GitHub pull request (your git credentials and gh)
    X               stop all agents and the daemon (r starts it again)
    v               review: comparisons, files, Accept / Reject (e: your $EDITOR; F: Follow)
    M               merge back (asks each step)
    /               search agents              A     accounts and sign-in
    O               phone access on / off      ctrl+o devices: pair a phone, revoke, scope
    S               Audio Mode, track, preview  e     in zoom: expand tool calls
    f               filter all/active/needs you/archived   ?  all keys
    E               archive a finished agent (in the Archived filter: restore it)
    q               quit (agents keep running)

Top-level agents, newest first; the grid fits the count, up to sixteen: page 1 is the newest sixteen.";

enum Ev {
    Daemon(Msg),
    Term(Event),
}

fn main() -> Result<()> {
    let mut daemon_path: Option<PathBuf> = None;
    let mut home: Option<PathBuf> = None;
    let mut mouse = true;
    let mut bell = true;
    let mut dashboard = false;
    let mut grid_only = false;
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
            "--home" => home = args.next().map(PathBuf::from),
            "--no-mouse" => mouse = false,
            "--no-bell" => bell = false,
            "--dashboard" => dashboard = true,
            "--grid" => grid_only = true,
            other => anyhow::bail!("unknown option {other} (see --help)"),
        }
    }
    if dashboard && grid_only {
        anyhow::bail!("--dashboard and --grid are for two terminals: pick one per terminal");
    }
    let mut daemon = Daemon::find(daemon_path.as_deref())?;
    // An explicit --home is deliberate; OVERSEER_HOME in the environment only counts for a dev TUI (AC-212).
    daemon.home = home;
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
    let jobs = dtx.clone();
    let client = Arc::new(Client::start(Some(daemon), socket, dtx));
    let mut app = App::new(client.clone());
    app.set_jobs(jobs);
    app.cwd_repo = git_root();
    app.dashboard = dashboard;
    app.grid_only = grid_only;
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
    // Reads keys unless paused (while a sign-in owns the terminal, its keys are its own).
    let paused = Arc::new(AtomicBool::new(false));
    let input = tx.clone();
    let reader_paused = paused.clone();
    std::thread::spawn(move || loop {
        if reader_paused.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        match event::poll(Duration::from_millis(50)) {
            Ok(true) if !reader_paused.load(Ordering::SeqCst) => match event::read() {
                Ok(e) => {
                    if input.send(Ev::Term(e)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            },
            Ok(_) => {}
            Err(_) => break,
        }
    });

    let result = (|| -> Result<()> {
        let mut last_second = Instant::now();
        let mut title = String::new();
        loop {
            // The window title carries the counts (visible from another tab or window).
            let t = app.window_title();
            if t != title {
                let _ = execute!(std::io::stdout(), crossterm::terminal::SetTitle(&t));
                title = t;
            }
            if std::mem::take(&mut app.bell) && bell {
                use std::io::Write;
                let _ = std::io::stdout().write_all(b"\x07");
                let _ = std::io::stdout().flush();
            }
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
            if let Some(exec) = app.exec.take() {
                // Hand the terminal to the program (a provider's own sign-in), then come back.
                paused.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(120));
                if mouse {
                    let _ = execute!(std::io::stdout(), DisableMouseCapture);
                }
                let _ = execute!(std::io::stdout(), DisableBracketedPaste);
                ratatui::restore();
                if !exec.edit {
                    println!("{} — Overseer resumes when it finishes.\n", exec.title);
                }
                let status = std::process::Command::new(&exec.program).args(&exec.args).envs(exec.env.iter().map(|(k, v)| (k, v))).status();
                terminal = ratatui::init();
                let _ = execute!(std::io::stdout(), EnableBracketedPaste);
                if mouse {
                    let _ = execute!(std::io::stdout(), EnableMouseCapture);
                }
                // A plain clear: no cursor-position query, which not every terminal answers.
                let _ = execute!(std::io::stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All));
                paused.store(false, Ordering::SeqCst);
                app.dirty = true;
                app.after_exec(status.map(|s| s.code().unwrap_or(-1)).map_err(|e| e.to_string()));
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
