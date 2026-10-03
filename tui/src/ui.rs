//! Rendering: the header, the 3×3 page of agents, the composer, zoom, help and the New Agent
//! form. Terminal default colors for text (works on dark and light terminals) plus a purple
//! accent and status colors; truecolor when the terminal says so, 256 colors otherwise.

use crate::app::{code_groups, short, App, Confirm, Mode, NewAgentForm, PairingState};
use crate::qr;
use crate::feed::{compact, Feed, Item, Kind, ToolStatus};
use crate::model::Run;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::sync::atomic::{AtomicBool, Ordering};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static TRUECOLOR: AtomicBool = AtomicBool::new(false);

pub fn set_truecolor(on: bool) {
    TRUECOLOR.store(on, Ordering::Relaxed);
}

fn accent() -> Color {
    if TRUECOLOR.load(Ordering::Relaxed) {
        Color::Rgb(155, 123, 255)
    } else {
        Color::Indexed(141)
    }
}

fn waiting() -> Color {
    if TRUECOLOR.load(Ordering::Relaxed) {
        Color::Rgb(242, 168, 59)
    } else {
        Color::Indexed(214)
    }
}

const MUTED: Color = Color::DarkGray;

/// Status glyph and color.
pub fn status_mark(status: &str) -> (&'static str, Color) {
    match status {
        "queued" => ("○", MUTED),
        "starting" | "running" => ("●", Color::Cyan),
        "waiting_for_user" => ("◆", waiting()),
        "completed" => ("✓", Color::Green),
        "failed" => ("✗", Color::Red),
        "interrupted" => ("■", Color::Yellow),
        "disconnected" => ("⚡", Color::Red),
        // Continuity (Gate L): waiting is not a failure; a handed-off agent points at its successor.
        "waiting_for_connection" | "waiting_for_memory" => ("☁", waiting()),
        "handed_off" => ("→", MUTED),
        _ => ("?", Color::Magenta),
    }
}

fn status_word(status: &str) -> &str {
    match status {
        "waiting_for_user" => "needs you",
        "waiting_for_connection" => "waiting for a connection",
        "waiting_for_memory" => "waiting for memory",
        "handed_off" => "handed off",
        "starting" => "starting",
        s => s,
    }
}

/// "12s", "4m", "2h", "3d".
pub fn elapsed(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.stats.draws += 1;
    app.hit.clear();
    app.list_hit.clear();
    let area = f.area();
    if app.size != (area.width, area.height) {
        app.size = (area.width, area.height);
    }
    // The page follows the focused agent when the room changes (a resize, the list, a conversation).
    app.settle_focus();
    let composing = matches!(app.mode, Mode::Compose) || matches!(app.mode, Mode::Confirm(_));
    let composer_h = if matches!(app.mode, Mode::Compose) { composer_height(app, area.width) } else if matches!(app.mode, Mode::DenyNote { .. }) { 3 } else if composing { confirm_height(app, area.width) } else { 0 };
    let [head, body, comp, foot] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(composer_h), Constraint::Length(1)]).areas(area);
    header(f, app, head);
    match app.mode {
        Mode::Zoom { .. } => zoom(f, app, body),
        _ if app.dashboard_shown() && app.mode != Mode::Changes => dashboard(f, app, body),
        Mode::Changes | Mode::Confirm(Confirm::Reject { .. }) => changes(f, app, body, true),
        _ if app.compact() && !(app.state.runs.is_empty() || app.visible().is_empty()) => compact_layout(f, app, body),
        _ => main_screen(f, app, body),
    }
    if matches!(app.mode, Mode::Compose) {
        composer(f, app, comp);
    } else if let Mode::DenyNote { run, .. } = &app.mode {
        deny_note(f, app, run, comp);
    } else if let Mode::Confirm(c) = &app.mode {
        let text = match c {
            Confirm::Mods { text } => format!(" {} y / n", crate::app::mods_display(text)),
            Confirm::Interrupt(id) => format!(" Interrupt {}? y / n", short(&app.state.run(id).map(|r| r.title.clone()).unwrap_or_default(), 50)),
            Confirm::Quit => " Unsent drafts will be lost. Quit? y / n".to_string(),
            Confirm::MergePrepare { text, .. } | Confirm::MergeComplete { text, .. } | Confirm::MergeCancel { text, .. } | Confirm::Cleanup { text, .. } | Confirm::StopAll { text } | Confirm::Archive { text, .. } | Confirm::OpenPr { text, .. } | Confirm::Reject { text, .. } => format!(" {text} y / n"),
            Confirm::PhoneOff { text } | Confirm::Revoke { text, .. } | Confirm::Pair { text, .. } => format!(" {text} y / n"),
            Confirm::PhoneOnAndPair => " Phone access is off. Turn it on and pair a phone? y / n".to_string(),
        };
        f.render_widget(Paragraph::new(Line::from(Span::styled(text, Style::new().fg(waiting()).add_modifier(Modifier::BOLD)))).wrap(Wrap { trim: false }), comp);
    }
    footer(f, app, foot);
    // A phone question keeps its panel in view above it.
    let shown = match (&app.mode, &app.confirm_back) {
        (Mode::Confirm(Confirm::PhoneOff { .. } | Confirm::PhoneOnAndPair | Confirm::Revoke { .. } | Confirm::Pair { .. }), Some(back)) => back.clone(),
        (Mode::Confirm(Confirm::Mods { .. }), _) => Mode::Mods { run_id: app.mods.target.clone() },
        (mode, _) => mode.clone(),
    };
    match shown {
        Mode::Mods { .. } => mods_view(f, app, Rect { height: head.height + body.height, ..area }),
        Mode::Help => help(f, area),
        Mode::NewAgent => new_agent(f, &app.form, &app.state, area),
        Mode::Accounts => accounts(f, app, area),
        Mode::Devices => devices(f, app, Rect { y: head.y, height: head.height + body.height, ..area }),
        Mode::Pairing => pairing(f, app, Rect { y: head.y, height: head.height + body.height, ..area }),
        Mode::Audio => audio_mode(f, app, area),
        Mode::AudioImport => audio_import(f, app, area),
        Mode::Overseer => overseer_view(f, app, area),
        Mode::Queue => queue_view(f, app, area),
        _ => {}
    }
}

/// Owner controls for the shared daemon queue. No refresh, focus change or reconnect resumes it.
fn queue_view(f: &mut Frame, app: &App, area: Rect) {
    let Some(run) = app.focused() else { return };
    let w = area.width.saturating_sub(4).min(100);
    let h = area.height.saturating_sub(4).min(24);
    let r = Rect { x: area.x + area.width.saturating_sub(w)/2, y: area.y + area.height.saturating_sub(h)/2, width:w, height:h };
    let paused = run.queue["paused"] == true;
    let block = Block::default().borders(Borders::ALL).title(format!(" {} · {} ", run.title, if paused { "Queue paused" } else { "Queued" })).border_style(Style::new().fg(accent()));
    let inner = block.inner(r);
    f.render_widget(Clear, r); f.render_widget(block, r);
    let messages = run.queue["messages"].as_array().cloned().unwrap_or_default();
    let mut lines = vec![Line::from(Span::styled("s Send queued   c Clear   d Remove   j/k select   Esc close", Style::new().fg(accent()))), Line::raw("")];
    if messages.is_empty() { lines.push(Line::raw("No queued messages")); }
    let visible = (inner.height as usize).saturating_sub(2).max(1);
    let cursor = app.queue_cursor.min(messages.len().saturating_sub(1));
    let start = cursor.saturating_sub(visible.saturating_sub(1));
    for (i,m) in messages.iter().enumerate().skip(start).take(visible) {
        let text = m["text"].as_str().unwrap_or("").trim_start_matches("From Overseer: ");
        let row = format!("{} {}. {}{}", if i == cursor { "›" } else { " " }, i+1, if paused { "Paused " } else { "" }, text);
        lines.push(Line::from(Span::styled(fit(&row, inner.width as usize), if i == cursor { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new().fg(MUTED) })));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The conversation with Overseer (AC-199): the owner's words, Overseer's replies, the daemon's
/// cards, the proposals that wait (answered with ctrl+y / ctrl+n), and what is being typed.
fn overseer_view(f: &mut Frame, app: &App, area: Rect) {
    let w = 110.min(area.width.saturating_sub(4));
    let h = area.height.saturating_sub(2).max(8);
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let inner_w = (w as usize).saturating_sub(4);
    let s = &app.overseer;
    let level = match s["level"].as_str().unwrap_or("ask_first") { "steer" => "Steer", "auto" => "Auto", _ => "Ask first" };
    let mut lines: Vec<Line> = Vec::new();
    let wrap = |text: &str, prefix: &str, style: Style, out: &mut Vec<Line>| {
        let mut first = true;
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                if !line.is_empty() && line.width() + word.width() + 1 > inner_w.saturating_sub(prefix.width()) {
                    out.push(Line::from(vec![Span::styled(if first { prefix.to_string() } else { " ".repeat(prefix.width()) }, Style::new().fg(MUTED)), Span::styled(std::mem::take(&mut line), style)]));
                    first = false;
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
            out.push(Line::from(vec![Span::styled(if first { prefix.to_string() } else { " ".repeat(prefix.width()) }, Style::new().fg(MUTED)), Span::styled(line, style)]));
            first = false;
        }
    };
    // Spoken requests (T-35) are the same cards as typed ones: the words, then how far they got.
    let spoken_of = |m: &serde_json::Value| -> Option<(String, String)> {
        if m["source"] != "owner" || m["surface"] != "voice" {
            return None;
        }
        let text = m["text"].as_str()?;
        let at = text.rfind("Request V-")?;
        let rest = &text[at + "Request ".len()..];
        let (id, words) = rest.split_once(": ")?;
        Some((id.to_string(), words.to_string()))
    };
    let messages = s["messages"].as_array().cloned().unwrap_or_default();
    let in_conversation: Vec<String> = messages.iter().filter_map(|m| spoken_of(m).map(|x| x.0)).collect();
    let mut timeline: Vec<serde_json::Value> = messages.clone();
    for r in &app.voice_requests {
        if !in_conversation.iter().any(|id| r["id"].as_str() == Some(id.as_str())) {
            timeline.push(serde_json::json!({ "source": "owner", "surface": "voice", "ts": r["ts"], "voice": r }));
        }
    }
    timeline.sort_by_key(|m| m["ts"].as_i64().unwrap_or(0));
    let request_of = |id: &str| app.voice_requests.iter().find(|r| r["id"].as_str() == Some(id)).cloned();
    for m in timeline {
        let source = m["source"].as_str().unwrap_or("system");
        let text = m["text"].as_str().unwrap_or("");
        let spoken = if m["voice"].is_object() { Some((m["voice"]["words"].as_str().unwrap_or("").to_string(), Some(m["voice"].clone()))) } else { spoken_of(&m).map(|(id, words)| (words, request_of(&id))) };
        if let Some((words, request)) = spoken {
            wrap(&words, " you (spoken) › ", Style::new().add_modifier(Modifier::BOLD), &mut lines);
            if let Some((stage, color)) = request.as_ref().and_then(voice_stage) {
                lines.push(Line::from(Span::styled(format!("   ◇ {stage}"), Style::new().fg(color))));
            }
            continue;
        }
        match source {
            // A spoken request: the owner's words, not Overseer's notes or the request's id (AC-219).
            "owner" => match crate::words::spoken(text) {
                Some(words) => wrap(words, " you (by voice) › ", Style::new().add_modifier(Modifier::BOLD), &mut lines),
                None => wrap(text, " you › ", Style::new().add_modifier(Modifier::BOLD), &mut lines),
            },
            // Overseer's replies are Markdown (AC-245): bullets, bold and headings as a terminal draws them.
            "overseer" => {
                let mut first = true;
                for raw in text.split('\n') {
                    let (line, heading) = crate::words::markdown_line(&crate::words::states(raw));
                    let style = if heading { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
                    wrap(&line, if first { " ◆ " } else { "   " }, style, &mut lines);
                    first = false;
                }
            }
            // A card: its mark and its words, never its internal kind.
            _ => {
                let kind = m["card"]["kind"].as_str().unwrap_or(source);
                let mark = match kind { "finding" | "cannot_answer" => " ⚠ ", "done" => " ✓ ", "started" => " ▶ ", _ => " · " };
                wrap(&crate::words::plain(text), mark, Style::new().fg(MUTED), &mut lines);
            }
        }
    }
    for p in s["proposals"].as_array().cloned().unwrap_or_default() {
        if p["state"] != "open" {
            continue;
        }
        lines.push(Line::from(Span::styled(" Overseer will:", Style::new().fg(waiting()).add_modifier(Modifier::BOLD))));
        for l in p["lines"].as_array().cloned().unwrap_or_default() {
            wrap(l.as_str().unwrap_or(""), "   • ", Style::new().fg(waiting()), &mut lines);
        }
        lines.push(Line::from(Span::styled("   ctrl+y yes · ctrl+n no", Style::new().fg(MUTED))));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" Nothing yet. Type to Overseer: what your agents are doing, or what one of them should do next.", Style::new().fg(MUTED))));
    }
    let draft_h = (app.overseer_draft.lines().count().max(1) as u16).min(4) + 1;
    let voice = voice_line(app, inner_w);
    let body_h = (h as usize).saturating_sub(2 + draft_h as usize + voice.len());
    let end = lines.len().saturating_sub(app.overseer_scroll.min(lines.len()));
    let start = end.saturating_sub(body_h);
    let shown: Vec<Line> = lines[start..end].to_vec();
    let mut all = voice;
    all.extend(shown);
    all.push(Line::from(Span::styled(" ".repeat(inner_w.min(120)).replace(' ', "─"), Style::new().fg(MUTED))));
    let draft = if app.overseer_draft.is_empty() { " › type a message for Overseer".to_string() } else { format!(" › {}", app.overseer_draft) };
    all.push(Line::from(Span::styled(draft, if app.overseer_draft.is_empty() { Style::new().fg(MUTED) } else { Style::new().fg(accent()) })));
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent()))
        .title(Span::styled(format!(" ◆ Overseer · {level} "), Style::new().add_modifier(Modifier::BOLD)))
        .title_bottom(Line::from(Span::styled(" enter sends · ctrl+y / ctrl+n yes / no · ctrl+v voice · esc closes ", Style::new().fg(MUTED))).right_aligned());
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(all).block(block).wrap(Wrap { trim: false }), r);
}

/// Voice Mode's words for its states, as VS Code's (extension/src/voice.js `LABEL`).
pub fn voice_label(state: &str) -> &'static str {
    match state {
        "off" => "Voice off",
        "starting" => "Starting",
        "listening" => "Listening",
        "hearing" => "Hearing you",
        "thinking" => "Thinking",
        "speaking" => "Speaking",
        "muted" => "Muted",
        "paused" => "Paused for a call",
        "failed" => "Voice stopped",
        _ => "Voice Mode",
    }
}

/// A spoken request's stage, in VS Code's words (extension/media/home.js `VOICE_STAGE`).
fn voice_stage(r: &serde_json::Value) -> Option<(String, Color)> {
    let (stage, text) = match r["state"].as_str()? {
        "taken" | "thinking" => ("thinking", "Overseer is thinking"),
        "settling" => ("sending", "Going out in a moment (ctrl+x cancels)"),
        "waiting" => ("waiting", "Waits for your yes (ctrl+y / ctrl+n)"),
        "waiting_turn" => ("thinking", "Waits for the requests before it"),
        "sent" => ("done", "Sent"),
        "partly_sent" => ("failed", "Partly sent: the card says what"),
        "cancelled" => ("done", "Cancelled: nothing was sent"),
        "corrected" => ("done", "Corrected by what you said next"),
        "superseded" => ("done", "Replaced by your correction"),
        "joined" => ("done", "Joined with what you said next"),
        "not_sent" => ("failed", "Not sent"),
        "answered" => ("done", "Answered"),
        "done" => ("done", "Done"),
        "not_for_overseer" => ("aside", "Not meant for Overseer: kept as context"),
        _ => return None,
    };
    let text = match r["done"].as_str() {
        Some(d) if !d.is_empty() && matches!(r["state"].as_str(), Some("sent" | "not_sent" | "done" | "partly_sent")) => fit(&crate::words::plain(d).replace('\n', " "), 100),
        _ => text.to_string(),
    };
    let color = match stage { "failed" => Color::Red, "waiting" => waiting(), "done" => Color::Green, _ => accent() };
    Some((text, color))
}

/// The voice line on top of Overseer's conversation (T-35): the state, who it talks to, the words
/// as they are heard, and the keys. The daemon listens and speaks; the terminal only shows.
fn voice_line(app: &App, width: usize) -> Vec<Line<'static>> {
    let Some(state) = app.voice_state() else {
        return vec![Line::from(Span::styled(" ◎ Voice Mode is unavailable with this daemon", Style::new().fg(MUTED)))];
    };
    let key = |k: &'static str| Span::styled(k, Style::new().fg(accent()).add_modifier(Modifier::BOLD));
    let word = |w: &'static str| Span::styled(w, Style::new().fg(MUTED));
    if state == "off" {
        let why = app.voice["reason"].as_str().filter(|r| !r.is_empty()).map(|r| format!(" ({})", crate::words::plain(r))).unwrap_or_default();
        return vec![Line::from(vec![Span::styled(format!(" ◎ Voice off{why}  "), Style::new().fg(MUTED)), key("ctrl+v"), word(" turns it on")]), Line::raw("")];
    }
    let color = match state { "hearing" => Color::Green, "speaking" | "thinking" => accent(), "muted" | "paused" => waiting(), "failed" => Color::Red, _ => Color::Cyan };
    let target = match app.voice["target"].as_str() {
        None | Some("overseer") | Some("") => "Overseer".to_string(),
        Some(id) => app.state.run(id).map(|r| r.title.clone()).unwrap_or_else(|| id.to_string()),
    };
    let reason = app.voice["reason"].as_str().filter(|r| !r.is_empty()).map(|r| format!(" ({})", crate::words::plain(r))).unwrap_or_default();
    let mut out = vec![Line::from(vec![
        Span::styled(format!(" ◉ {}", voice_label(state)), Style::new().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{reason} · talking to {} ", fit(&target, 30)), Style::new().fg(MUTED)),
        key(" ctrl+v"), word(" off "), key("ctrl+t"), word(if state == "muted" { " unmute " } else { " mute " }), key("ctrl+a"), word(" talk to "), key("ctrl+x"), word(" cancel"),
    ])];
    if !app.voice_heard.is_empty() {
        out.push(Line::from(vec![Span::styled("   heard: ", Style::new().fg(MUTED)), Span::styled(format!("“{}”", fit(&app.voice_heard, width.saturating_sub(14))), Style::new().add_modifier(Modifier::ITALIC))]));
    }
    if app.voice_asking {
        out.push(Line::from(vec![Span::styled("   waits for your yes  ", Style::new().fg(waiting()).add_modifier(Modifier::BOLD)), key("ctrl+y"), word(" yes "), key("ctrl+n"), word(" no")]));
    }
    out.push(Line::raw(""));
    out
}

fn header(f: &mut Frame, app: &App, area: Rect) {
    let visible = app.visible();
    let dot = Style::new().fg(MUTED);
    let mut spans = vec![
        Span::styled(" ◆ Overseer ", Style::new().fg(accent()).add_modifier(Modifier::BOLD)),
        if app.dashboard_shown() {
            Span::styled(" dashboard ", Style::new().add_modifier(Modifier::BOLD))
        } else if app.grid_only {
            Span::styled(format!(" grid only · page {}/{} ", app.page + 1, app.pages()), Style::new().add_modifier(Modifier::BOLD))
        } else {
            Span::styled(format!(" page {}/{} ", app.page + 1, app.pages()), Style::new().add_modifier(Modifier::BOLD))
        },
        Span::styled("· ", dot),
        Span::raw(format!("{} agent{}", visible.len(), if visible.len() == 1 { "" } else { "s" })),
    ];
    // The rollup (T-26): VS Code's five counts, zero counts left out; on a narrow terminal the
    // later ones give way to the connection on the right.
    let room = (area.width as usize).saturating_sub(24);
    for (key, n, words) in app.state.counts(crate::model::now_ms()).parts() {
        if Line::from(spans.clone()).width() + 3 + words.width() + 2 > room {
            break;
        }
        spans.push(Span::styled(" · ", dot));
        spans.push(match key {
            "working" => Span::styled(format!("● {words}"), Style::new().fg(Color::Cyan)),
            "needs" => Span::styled(format!("◆ {n} need{} you", if n == 1 { "s" } else { "" }), Style::new().fg(waiting()).add_modifier(Modifier::BOLD)),
            "to_review" => Span::styled(format!("✦ {words}"), Style::new().fg(Color::Green)),
            "failed" => Span::styled(format!("✗ {words}"), Style::new().fg(Color::Red)),
            _ => Span::styled(words, dot),
        });
    }
    if !app.search.is_empty() || app.mode == Mode::Search {
        spans.push(Span::styled(" · ", dot));
        spans.push(Span::styled(format!("/{}", app.search), Style::new().fg(accent()).add_modifier(Modifier::BOLD)));
        if app.mode == Mode::Search {
            spans.push(Span::styled("▌", Style::new().fg(accent())));
        }
    }
    if app.filter != crate::app::Filter::All {
        spans.push(Span::styled(" · ", dot));
        spans.push(Span::styled(format!("filter: {}", app.filter.label()), Style::new().fg(accent())));
    }
    let left = Line::from(spans);
    let mut right = Vec::new();
    // Phone access, at a glance: the long form when it fits beside the counts, else the short one.
    if let Some((long, brief, on)) = app.phone.line().filter(|_| app.connected) {
        let room = (area.width as usize).saturating_sub(left.width() + "● connected ".width() + 4);
        let text = if long.width() <= room { Some(long) } else if brief.width() <= room { Some(brief) } else { None };
        if let Some(text) = text {
            right.push(Span::styled(text, if on { Style::new().fg(accent()) } else { Style::new().fg(MUTED) }));
            right.push(Span::styled(" · ", dot));
        }
    }
    right.push(if app.connected {
        Span::styled("● connected ", Style::new().fg(Color::Green))
    } else if app.stopped {
        Span::styled("○ stopped · r starts ", Style::new().fg(MUTED))
    } else {
        Span::styled("○ reconnecting ", Style::new().fg(Color::Red))
    });
    f.render_widget(Paragraph::new(left), area);
    f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right), area);
}

fn footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some((text, _, error)) = &app.notice {
        let style = if *error { Style::new().fg(Color::Red) } else { Style::new().fg(accent()) };
        f.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))), area);
        return;
    }
    let keys: &[(&str, &str)] = match app.mode {
        Mode::Confirm(Confirm::Mods { .. }) => &[("y", "confirm"), ("n / esc", "cancel")],
        Mode::Mods { .. } if matches!(app.mods.form, Some(crate::app::ModsForm::Source { .. })) => &[("type", "source"), ("enter", "preview"), ("ctrl+u", "clear"), ("esc", "cancel")],
        Mode::Mods { .. } if matches!(app.mods.form, Some(crate::app::ModsForm::Binding { .. })) => &[("tab", "field"), ("arrows", "choose"), ("space", "toggle"), ("enter", "review"), ("esc", "cancel")],
        Mode::Mods { .. } if matches!(app.mods.form, Some(crate::app::ModsForm::Filter { .. })) => &[("a", "add identifier"), ("x", "remove"), ("enter", "add"), ("esc", "back")],
        Mode::Mods { .. } if matches!(app.mods.form, Some(crate::app::ModsForm::Target { .. })) => &[("j/k", "agent"), ("enter", "inspect"), ("esc", "cancel")],
        Mode::Mods { .. } if app.mods.preview.is_some() => &[("enter", "review install"), ("pgup/pgdn", "scroll"), ("esc", "close preview")],
        Mode::Mods { .. } => &[("tab", "Applied/Library"), ("r", "refresh"), ("?", "help"), ("esc", "back")],
        Mode::Queue => &[("s", "Send queued"), ("c", "Clear"), ("d", "Remove"), ("j/k", "select"), ("esc", "close")],
        Mode::DenyNote { .. } => &[("type", "a note for the agent"), ("enter", "deny"), ("esc", "cancel")],
        Mode::Compose if app.focused().is_some_and(|r| r.permission_request().is_some()) => &[("enter", "deny with this note"), ("alt+enter", "new line"), ("esc", "close (keeps draft)"), ("ctrl+u", "clear")],
        Mode::Compose => &[("enter", "send"), ("alt+enter", "new line"), ("esc", "close (keeps draft)"), ("ctrl+u", "clear")],
        Mode::Zoom { .. } => &[("e", if app.expand_tools { "fold tools" } else { "expand tools" }), ("j/k", "scroll"), ("home/G", "top/bottom"), ("i", "message"), ("a/s/d", "allow once/session/deny"), ("x", "interrupt"), ("g/z", "grid"), ("?", "help")],
        Mode::NewAgent => &[("tab", "next field"), ("←/→", "choose"), ("enter", "start"), ("esc", "cancel")],
        Mode::Accounts => &[("j/k", "select"), ("s", "sign in"), ("S", "device code (ChatGPT)"), ("r", "refresh"), ("esc", "close")],
        Mode::Devices => &[("p", "pair a phone"), ("j/k", "select"), ("s", "scope"), ("x", "revoke"), ("N", "notifications"), ("O", "on/off"), ("esc", "close")],
        Mode::Pairing if app.pairing.as_ref().is_some_and(|p| p.live()) => &[("esc", "cancel pairing")],
        Mode::Pairing => &[("p", "new code"), ("esc", "close")],
        Mode::Audio => &[("space", "on/off"), ("1/2/3", "track"), ("tab", "cue"), ("p", "preview"), ("v", "voice"), ("i", "import"), ("esc", "close")],
        Mode::AudioImport => &[("type", "private folder path"), ("enter", "import"), ("esc", "back")],
        Mode::Overseer => &[("type", "to Overseer"), ("enter", "send"), ("ctrl+y/n", "yes/no"), ("ctrl+v", "voice on/off"), ("ctrl+t", "mute"), ("ctrl+a", "talk to"), ("ctrl+x", "cancel"), ("esc", "close")],
        Mode::Search => &[("type", "to search title, repo, harness, model, prompt"), ("enter", "keep"), ("esc", "clear")],
        Mode::Changes => &[("F", "Follow"), ("n/p", "change"), ("a/A", "Accept change/file"), ("r/R", "Reject change/file"), ("j/k", "file"), ("t", "Changed | All files"), ("1/2/3 c", "comparison"), ("e", "your editor"), ("J/K", "scroll"), ("ctrl+r", "reload"), ("esc", "back")],
        Mode::Grid if app.dashboard_shown() && app.dash_col == 0 => &[("J/K j/k", "next / previous agent"), ("tab", "review, conversation"), ("i", "message"), ("a/s/d", "allow once/session/deny"), ("g", "full view"), ("w", "next waiting"), ("n", "new"), ("D", "grid"), ("?", "help"), ("q", "quit")],
        Mode::Grid if app.dashboard_shown() && app.dash_col == 1 => &[("J/K", "agent"), ("F", "Follow"), ("n/p", "change"), ("a/A", "Accept change/file"), ("r/R", "Reject change/file"), ("j/k", "file"), ("t", "Changed | All files"), ("1/2/3 c", "comparison"), ("e", "your editor"), ("pgup/pgdn", "scroll"), ("tab", "conversation"), ("D", "grid")],
        Mode::Grid if app.dashboard_shown() => &[("J/K", "agent"), ("j/k pgup/pgdn", "scroll"), ("e", if app.expand_tools { "fold tools" } else { "tool details" }), ("i", "message"), ("a/s/d", "allow once/session/deny"), ("tab", "list"), ("D", "grid"), ("?", "help")],
        Mode::Grid if app.picked && app.focused().is_some() => &[("J/K", "next / previous agent"), ("esc", "close the conversation"), ("pgup/pgdn", "scroll"), ("e", if app.expand_tools { "fold tools" } else { "tool details" }), ("i", "message"), ("v", "changes"), ("?", "help")],
        _ if area.width < 110 => &[("i", "message"), ("g", "full view"), ("a/d", "answer"), ("n", "new"), ("o", "Overseer"), ("?", "keys"), ("q", "quit")],
        _ if app.grid_only => &[("←↑↓→", "move"), ("tab", "next agent"), ("i", "message"), ("g", "full view"), ("v", "changes"), ("a/s/d", "allow once/session/deny"), ("w", "next waiting"), ("]/[", "page"), ("n", "new"), ("f", "filter"), ("?", "help"), ("q", "quit")],
        _ => &[("←↑↓→", "move"), ("J/K", "pick in the list"), ("i", "message"), ("g", "full view"), ("v", "changes"), ("a/s/d", "allow once/session/deny"), ("w", "next waiting"), ("]/[", "page"), ("n", "new"), ("f", "filter"), ("?", "help"), ("q", "quit")],
    };
    // Dashboard mode asked for (`--dashboard`) in a terminal too narrow for it: say so, on the grid.
    if app.dashboard && !app.dashboard_shown() && matches!(app.mode, Mode::Grid) {
        let text = format!(" Dashboard mode needs a terminal at least {} columns wide (this one is {}); showing the grid", crate::app::DASHBOARD_MIN_W, app.size.0);
        f.render_widget(Paragraph::new(Line::from(vec![Span::styled(text, Style::new().fg(waiting())), Span::styled("   D", Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::styled(" grid only", Style::new().fg(MUTED))])), area);
        return;
    }
    let mut spans = vec![Span::raw(" ")];
    for (k, v) in keys {
        spans.push(Span::styled(*k, Style::new().fg(accent()).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {v}   "), Style::new().fg(MUTED)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn empty(f: &mut Frame, app: &App, area: Rect) {
    let msg = if app.state.runs.is_empty() {
        if app.connected { "No agents yet. Press n to start one." } else { "Connecting to overseerd…" }
    } else {
        "No agents match. Press f to change the filter, or esc to clear the search."
    };
    let y = area.y + area.height / 2;
    f.render_widget(Paragraph::new(Line::from(Span::styled(msg, Style::new().fg(MUTED)))).alignment(Alignment::Center), Rect { y, height: 1, ..area });
}

/// The main screen (T-25): the agent list on the left, the grid, and the picked agent's
/// conversation in a column beside the grid (way 2, the owner's pick).
fn main_screen(f: &mut Frame, app: &mut App, area: Rect) {
    let (list_w, conv_w) = crate::app::side_widths(app.size.0, app.list_shown(), app.picked && app.focused().is_some());
    let [list, rest, conv] = Layout::horizontal([Constraint::Length(list_w), Constraint::Min(10), Constraint::Length(conv_w)]).areas(area);
    if list_w > 0 {
        agent_list(f, app, list);
    }
    if app.state.runs.is_empty() || app.visible().is_empty() {
        empty(f, app, rest);
    } else {
        grid(f, app, rest);
    }
    if conv_w > 0 {
        if let Some(run) = app.focused().cloned() {
            let slot = app.page_agents().iter().position(|r| r.id == run.id).map(|i| i + 1).unwrap_or(0);
            tile(f, app, &run, slot, conv, true);
        }
    }
}

/// Dashboard mode (T-40): the agent list on the left, the picked agent's review in the middle
/// and its conversation on the right, like the Overseer layout in VS Code (AC-264). The column
/// that takes the keys (Tab) is drawn as focused.
fn dashboard(f: &mut Frame, app: &mut App, area: Rect) {
    let (list_w, conv_w) = dashboard_widths(area.width, app.list_shown());
    let [list, mid, conv] = Layout::horizontal([Constraint::Length(list_w), Constraint::Min(30), Constraint::Length(conv_w)]).areas(area);
    app.dash_cols = [(list.x, list.width), (mid.x, mid.width), (conv.x, conv.width)];
    if list_w > 0 {
        agent_list(f, app, list);
    }
    let Some(run) = app.focused().cloned() else {
        return empty(f, app, Rect { x: mid.x, width: mid.width + conv.width, ..mid });
    };
    if app.changes.run == run.id {
        changes(f, app, mid, app.dash_col == 1);
    } else {
        let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(MUTED)).title(Span::styled(" review ", Style::new().fg(MUTED)));
        f.render_widget(Paragraph::new(Span::styled(" loading…", Style::new().fg(MUTED))).block(block), mid);
    }
    tile(f, app, &run, 0, conv, true);
}

/// Dashboard mode's list and conversation widths for a terminal `width` columns wide; the review
/// takes the rest (at 160 columns: 32, 77 and 51).
pub fn dashboard_widths(width: u16, list: bool) -> (u16, u16) {
    let list_w = if list { (width / 5).clamp(30, 40) } else { 0 };
    let conv_w = (width.saturating_sub(list_w) * 2 / 5).clamp(48, 84);
    (list_w, conv_w)
}

/// The account in a few words for a list row: a named account's name, else the plan
/// ("Claude Max") of the Mac's default login.
fn account_word(app: &App, run: &Run) -> String {
    let Some(p) = run.profile_id.as_deref().and_then(|p| app.state.profile(p)) else { return String::new() };
    if p.is_system {
        p.short().split(" · ").next().unwrap_or_default().to_string()
    } else {
        crate::words::account(&p.name)
    }
}

/// A list row's mark: needs you, to review (T-26), or what the work became (merged, a pull
/// request, stopped on conflicts).
fn row_mark(app: &App, run: &Run, now: i64) -> (&'static str, Color) {
    if run.needs_you() {
        return ("◆", waiting());
    }
    if app.state.unreviewed(run, now) {
        return ("✦", if crate::model::FAILED.contains(&run.status.as_str()) { Color::Red } else { Color::Green });
    }
    match app.state.landings[run.workspace_id.as_str()]["state"].as_str() {
        Some("merged") if !run.active() => ("✓", Color::Green),
        Some("pr") if !run.active() => ("↗", accent()),
        Some("conflicts") => ("⚠", waiting()),
        _ => (" ", MUTED),
    }
}

/// The agent list (T-25): grouped by repository with each repository's counts; each row the
/// status, the title, the account and a mark.
fn agent_list(f: &mut Frame, app: &mut App, area: Rect) {
    // The marks' legend, in short words when the list is narrow.
    let legend = [" ◆ needs you ✦ to review ✓ merged ", " ◆ you ✦ review ✓ merged ", " ◆ ✦ ✓ "].into_iter().find(|l| l.width() + 2 <= area.width as usize).unwrap_or("");
    // In dashboard mode the list is drawn as focused while it takes the keys (T-40).
    let active = app.dashboard_shown() && app.dash_col == 0;
    let (border, kind) = if active { (Style::new().fg(accent()).add_modifier(Modifier::BOLD), BorderType::Thick) } else { (Style::new().fg(MUTED), BorderType::Rounded) };
    let block = Block::bordered().border_type(kind).border_style(border).title(Span::styled(" agents ", if active { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new().fg(MUTED) }))
        .title_bottom(Line::from(Span::styled(legend, Style::new().fg(MUTED))).right_aligned());
    let now = crate::model::now_ms();
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut rows: Vec<(usize, String)> = Vec::new();
    let mut focused_line = 0;
    for (repo, runs) in app.groups() {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        let name = repo.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("(no repository)").to_string();
        let count = |pred: &dyn Fn(&Run) -> bool| runs.iter().filter(|r| pred(r)).count();
        let mut counts = Vec::new();
        for (n, word) in [
            (count(&|r: &Run| r.active() && !r.needs_you()), "working"),
            (count(&|r: &Run| r.needs_you()), "needs you"),
            (count(&|r: &Run| app.state.unreviewed(r, now) && !crate::model::FAILED.contains(&r.status.as_str())), "to review"),
            (count(&|r: &Run| app.state.unreviewed(r, now) && crate::model::FAILED.contains(&r.status.as_str())), "failed"),
            (count(&|r: &Run| app.state.landings[r.workspace_id.as_str()]["state"] == "merged"), "merged"),
        ] {
            if n > 0 {
                counts.push(format!("{n} {word}"));
            }
        }
        let head = fit(&name, w.saturating_sub(2).min(24));
        let counts = counts.join(" · ");
        let bold = Style::new().add_modifier(Modifier::BOLD);
        if head.width() + counts.width() + 3 <= w {
            lines.push(Line::from(vec![Span::styled(format!(" {head}"), bold), Span::styled(format!("  {counts}"), Style::new().fg(MUTED))]));
        } else {
            // A narrow list: the counts go under the repository's name.
            lines.push(Line::from(Span::styled(format!(" {head}"), bold)));
            lines.push(Line::from(Span::styled(format!("   {}", fit(&counts, w.saturating_sub(3))), Style::new().fg(MUTED))));
        }
        for r in runs {
            let (g, c) = status_mark(&r.status);
            let focused = app.focus.as_deref() == Some(r.id.as_str());
            let (mark, mc) = row_mark(app, r, now);
            let account = fit(&account_word(app, r), 12);
            let title_w = w.saturating_sub(7 + account.width()).max(4);
            let title = fit(&r.title, title_w);
            let pad = w.saturating_sub(3 + title.width() + account.width() + 3);
            let style = if focused && app.picked { Style::new().fg(accent()).add_modifier(Modifier::BOLD | Modifier::REVERSED) } else if focused { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new() };
            if focused {
                focused_line = lines.len();
            }
            rows.push((lines.len(), r.id.clone()));
            lines.push(Line::from(vec![
                Span::styled(if focused { "›" } else { " " }, Style::new().fg(accent())),
                Span::styled(format!("{g} "), Style::new().fg(c)),
                Span::styled(title, style),
                Span::raw(" ".repeat(pad)),
                Span::styled(account, Style::new().fg(MUTED)),
                Span::styled(format!(" {mark} "), Style::new().fg(mc)),
            ]));
        }
    }
    let h = inner.height as usize;
    let skip = if lines.len() <= h { 0 } else { focused_line.saturating_sub(h / 2).min(lines.len() - h) };
    for (line, id) in rows {
        if line >= skip && line < skip + h {
            app.list_hit.push((id, inner.x, inner.y + (line - skip) as u16, inner.width, 1));
        }
    }
    f.render_widget(Paragraph::new(lines.into_iter().skip(skip).take(h).collect::<Vec<_>>()), inner);
}

fn grid(f: &mut Frame, app: &mut App, area: Rect) {
    let agents: Vec<Run> = app.page_agents().into_iter().cloned().collect();
    let (nr, nc) = app.grid_shape(agents.len());
    let rows = Layout::vertical(vec![Constraint::Ratio(1, nr as u32); nr]).split(area);
    for (slot, run) in agents.iter().enumerate() {
        let cols = Layout::horizontal(vec![Constraint::Ratio(1, nc as u32); nc]).split(rows[slot / nc]);
        tile(f, app, run, slot + 1, cols[slot % nc], false);
    }
}

/// Small terminals: a compact list of the page plus the focused agent.
fn compact_layout(f: &mut Frame, app: &mut App, area: Rect) {
    let list_w = (area.width * 2 / 5).clamp(24, 44).min(area.width.saturating_sub(20));
    let [list, right] = Layout::horizontal([Constraint::Length(list_w), Constraint::Min(10)]).areas(area);
    let agents: Vec<Run> = app.page_agents().into_iter().cloned().collect();
    let mut lines = Vec::new();
    for (i, r) in agents.iter().enumerate() {
        let (g, c) = status_mark(&r.status);
        let focused = app.focus.as_deref() == Some(r.id.as_str());
        let style = if focused { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new() };
        let w = list_w.saturating_sub(7) as usize;
        lines.push(Line::from(vec![Span::styled(format!(" {} ", i + 1), Style::new().fg(MUTED)), Span::styled(format!("{g} "), Style::new().fg(c)), Span::styled(fit(&r.title, w), style)]));
        app.hit.push((r.id.clone(), list.x, list.y + 1 + i as u16, list.width, 1));
    }
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(MUTED)).title(Span::styled(" agents ", Style::new().fg(MUTED)));
    f.render_widget(Paragraph::new(lines).block(block), list);
    if let Some(run) = app.focused().cloned() {
        let slot = agents.iter().position(|r| r.id == run.id).map(|i| i + 1).unwrap_or(0);
        tile(f, app, &run, slot, right, false);
    }
}

fn zoom(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(run) = app.focused().cloned() else { return empty(f, app, area) };
    let slot = app.page_agents().iter().position(|r| r.id == run.id).map(|i| i + 1).unwrap_or(0);
    tile(f, app, &run, slot, area, true);
}

fn tile(f: &mut Frame, app: &mut App, run: &Run, slot: usize, area: Rect, zoomed: bool) {
    // In dashboard mode the conversation column is drawn as focused only while it takes the keys.
    let dimmed = zoomed && app.dashboard_shown() && app.mode != Mode::Changes && !matches!(app.mode, Mode::Zoom { .. }) && app.dash_col != 2;
    let focused = app.focus.as_deref() == Some(run.id.as_str()) && !dimmed;
    let (glyph, color) = status_mark(&run.status);
    let border = if focused { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new().fg(MUTED) };
    // The account it runs on (AC-235): provider and plan, the shortened email; on the bottom
    // border, so the title keeps its room.
    let profile = run.profile_id.as_deref().and_then(|p| app.state.profile(p));
    let account = profile.map(|p| p.short()).unwrap_or_default();
    // Where the whole of it does not fit, the email alone still says which account.
    let email = profile.and_then(|p| p.account.as_ref()).and_then(|a| a.email.clone()).unwrap_or_default();
    let harness = crate::words::harness(&run.harness);
    let mut meta = vec![harness.clone()];
    if let Some(m) = run.model.as_deref().filter(|m| !m.is_empty()) {
        meta.push(m.to_string());
    }
    // Oversight (AC-199): held, watched, watching, in conflict, from the daemon's state.
    meta.extend(app.state.marks(&run.id));
    let age = elapsed(run.ended_ms.unwrap_or_else(now_ms) - run.created_ms);
    let mut right = format!(" {} · {} ", meta.join(" · "), age);
    // A narrow tile keeps its title and drops the harness and model (the conversation and zoom show them).
    if run.title.width() + right.width() + 8 > area.width as usize && !zoomed {
        right.clear();
    }
    let room = (area.width as usize).saturating_sub(right.width() + 8);
    let title_style = if focused { Style::new().add_modifier(Modifier::BOLD).fg(accent()) } else { Style::new().add_modifier(Modifier::BOLD) };
    let title = Line::from(vec![
        Span::styled(if slot > 0 { format!(" {slot} ") } else { " ".into() }, Style::new().fg(MUTED)),
        Span::styled(format!("{glyph} "), Style::new().fg(color)),
        Span::styled(fit(&run.title, room.max(8)), title_style),
        Span::raw(" "),
    ]);
    let mut block = Block::default().borders(Borders::ALL).border_type(if focused { BorderType::Thick } else { BorderType::Rounded }).border_style(border).title(title);
    if !right.is_empty() && area.width as usize > right.width() + 20 {
        block = block.title_top(Line::from(Span::styled(right, Style::new().fg(MUTED))).right_aligned());
    }
    // Bottom: what needs attention, a draft, or the status.
    let feed = app.feeds.get(&run.id);
    let pending = run.permission_request().is_some() || feed.and_then(|f| f.pending_permission()).is_some();
    let bottom = if pending {
        let what = feed.and_then(|f| f.pending_permission().map(|p| p.1.to_string())).or_else(|| run.attention.as_ref().and_then(|a| a["tool"].as_str().map(str::to_string))).unwrap_or_default();
        // T-31: Allow once, Allow for this session (when the harness offers its rule), Deny with a note.
        let session = run.attention.as_ref().is_some_and(|a| a["kind"] == "permission" && !a["always"].is_null());
        let key = |k: &'static str| Span::styled(k, Style::new().fg(accent()).add_modifier(Modifier::BOLD));
        let word = |w: &'static str| Span::styled(w, Style::new().fg(MUTED));
        let words = |long: bool| {
            let mut answers = vec![key("  a"), word(if long { " allow once " } else { " once " }), key("d"), word(" deny… ")];
            if session {
                answers.splice(2..2, [key("s"), word(if long { " this session " } else { " session " })]);
            }
            answers
        };
        // The short words when the long ones leave the request no room (a small tile).
        let long = (area.width as usize) >= Line::from(words(true)).width() + 4 + 14;
        let answers = words(long);
        let room = (area.width as usize).saturating_sub(Line::from(answers.clone()).width() + 4);
        let mut spans = vec![Span::styled(" ◆ ", Style::new().fg(waiting())), Span::styled(fit(&what, room.max(8)), Style::new().fg(waiting()).add_modifier(Modifier::BOLD))];
        spans.extend(answers);
        Some(Line::from(spans))
    } else if app.drafts.get(&run.id).is_some_and(|d| !d.trim().is_empty()) && !matches!(app.mode, Mode::Compose) {
        Some(Line::from(Span::styled(" ✎ draft ", Style::new().fg(accent()))))
    } else if let Some(landed) = app.state.landing_text(&run.workspace_id).filter(|_| !run.active()) {
        // AC-243: what the work became, "Merged into main (1a2b3c4)"; C cleans the worktree up.
        let merged = landed.starts_with("Merged");
        let mut spans = vec![Span::styled(format!(" {} {landed} ", if merged { "✓" } else if landed.starts_with("Merge stopped") { "⚠" } else { "↗" }), Style::new().fg(if merged { color } else { waiting() }))];
        if merged && app.state.workspace(&run.workspace_id).is_some_and(|w| w.kind == "worktree" && w.removed_ms.is_none()) {
            spans.push(Span::styled("C", Style::new().fg(accent()).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(" clean up ", Style::new().fg(MUTED)));
        }
        Some(Line::from(spans))
    } else if !run.active() {
        // "interrupted · by user (exit signal 2)": the reason without repeating the status.
        let word = run.exit_reason.as_deref().filter(|_| run.status != "completed").map(|r| {
            let r = crate::words::plain(r);
            let r = r.strip_prefix(status_word(&run.status)).map(str::trim_start).unwrap_or(&r);
            format!(" {} · {} ", status_word(&run.status), short(r, 40))
        }).unwrap_or_else(|| format!(" {} ", status_word(&run.status)));
        Some(Line::from(Span::styled(word, Style::new().fg(color))))
    } else {
        feed.filter(|f| f.tokens_in + f.tokens_out > 0).map(|f| Line::from(Span::styled(format!(" {} in / {} out ", compact(f.tokens_in), compact(f.tokens_out)), Style::new().fg(MUTED))))
    };
    let used = bottom.as_ref().map(|b| b.width()).unwrap_or(0);
    if let Some(b) = bottom {
        block = block.title_bottom(b);
    }
    if let Some(said) = [&account, &email].into_iter().find(|a| !a.is_empty() && (area.width as usize) >= used + a.width() + 6) {
        block = block.title_bottom(Line::from(Span::styled(format!(" {said} "), Style::new().fg(MUTED))).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    app.hit.push((run.id.clone(), area.x, area.y, area.width, area.height));
    let width = inner.width.saturating_sub(1) as usize;
    let lines = match (feed, zoomed) {
        (Some(feed), true) => {
            let all = all_lines(feed, width, app.expand_tools);
            let h = inner.height as usize;
            let max_scroll = all.len().saturating_sub(h);
            let scroll = match app.mode {
                Mode::Zoom { scroll } => scroll.min(max_scroll),
                _ => app.conv_scroll.min(max_scroll),
            };
            match &mut app.mode {
                Mode::Zoom { scroll: s } => *s = scroll,
                _ => app.conv_scroll = scroll,
            }
            let end = all.len() - scroll;
            all[end.saturating_sub(h)..end].to_vec()
        }
        (Some(feed), false) => tail_lines(feed, width, inner.height as usize),
        (None, _) => vec![Line::from(Span::styled("loading…", Style::new().fg(MUTED)))],
    };
    let mut lines = if lines.is_empty() {
        let prompt = app.state.task(&run.task_id).map(|t| t.prompt.clone()).unwrap_or_default();
        let mut out = Vec::new();
        if !prompt.trim().is_empty() {
            render_item(&Item { seq: 0, kind: Kind::User, text: prompt, child: None, detail: None }, width, &mut out);
        }
        out.push(Line::from(Span::styled(if run.active() { "working…" } else { "no output" }, Style::new().fg(MUTED))));
        out
    } else {
        lines
    };
    // Keep the whole paused queue visible below the conversation, in its stored order (AC-265).
    let messages = run.queue["messages"].as_array().cloned().unwrap_or_default();
    if run.queue["paused"] == true || !messages.is_empty() {
        let h = inner.height as usize;
        let shown = messages.len().min(h.saturating_sub(2));
        let room = h.saturating_sub(shown + 1);
        if lines.len() > room { lines = lines.split_off(lines.len() - room); }
        lines.push(Line::from(Span::styled(if run.queue["paused"] == true { "Queue paused · Q controls" } else { "Queued · Q controls" }, Style::new().fg(waiting()))));
        for (i, m) in messages.iter().take(shown).enumerate() {
            let text = m["text"].as_str().unwrap_or("").trim_start_matches("From Overseer: ");
            lines.push(Line::from(Span::styled(fit(&format!("{}. {}{}", i+1, if run.queue["paused"] == true { "Paused " } else { "" }, text), width), Style::new().fg(MUTED))));
        }
    }
    f.render_widget(Paragraph::new(lines), Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner });
}

/// The review (`v`, T-27 to T-29): the comparison in its header with the keys for the others,
/// Changed or All files, and the selected file's changes, each with Accept and Reject.
fn changes(f: &mut Frame, app: &mut App, area: Rect, active: bool) {
    let c = app.changes.clone();
    let run = app.state.run(&c.run).cloned().unwrap_or_default();
    let label = c.comparison().map(|o| o.label.as_str()).unwrap_or("…");
    let total: (u64, u64) = c.files.iter().fold((0, 0), |a, f| (a.0 + f.2, a.1 + f.3));
    let muted = Style::new().fg(MUTED);
    let title = Line::from(vec![
        Span::styled(" review ", Style::new().fg(accent()).add_modifier(Modifier::BOLD)),
        Span::styled("· ", muted),
        Span::styled(fit(&run.title, 50), Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(" · ", muted),
        Span::styled(label.to_string(), Style::new().fg(accent()).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" · {} file{} ", c.files.len(), if c.files.len() == 1 { "" } else { "s" }), muted),
        Span::styled(format!("+{} ", total.0), Style::new().fg(Color::Green)),
        Span::styled(format!("−{} ", total.1), Style::new().fg(Color::Red)),
    ]);
    // Follow (T-30): on, or paused by a move made by hand.
    let mut title = title;
    match c.follow {
        crate::app::Follow::On => title.spans.push(Span::styled("· ◉ Following the agent ", Style::new().fg(accent()).add_modifier(Modifier::BOLD))),
        crate::app::Follow::Paused => title.spans.push(Span::styled("· Follow Paused (F resumes) ", Style::new().fg(waiting()).add_modifier(Modifier::BOLD))),
        crate::app::Follow::Off => {}
    }
    let block = if active { Block::bordered().border_type(BorderType::Thick).border_style(Style::new().fg(accent())) } else { Block::bordered().border_type(BorderType::Rounded).border_style(muted) }.title(title);
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    // The comparisons one key away, and which file list is shown.
    let mut bar = vec![Span::raw(" ")];
    for (i, (mode, name)) in crate::app::REVIEW_KEYS.iter().enumerate() {
        let opt = c.options.iter().position(|o| &o.mode == mode);
        let current = opt == Some(c.option);
        let available = opt.is_some_and(|i| c.options[i].available);
        let style = if current { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else if available { Style::new() } else { muted };
        bar.push(Span::styled(format!("{}", i + 1), Style::new().fg(accent()).add_modifier(Modifier::BOLD)));
        bar.push(Span::styled(if current { format!(" [{name}]") } else if available || c.options.is_empty() { format!(" {name}") } else { format!(" {name} (not available)") }, style));
        bar.push(Span::raw("   "));
    }
    if !REVIEW_MODES.contains(&c.comparison().map(|o| o.mode.as_str()).unwrap_or("")) && !c.options.is_empty() {
        bar.push(Span::styled(format!("c [{label}]   "), Style::new().fg(accent()).add_modifier(Modifier::BOLD)));
    }
    bar.push(Span::styled("t ", Style::new().fg(accent()).add_modifier(Modifier::BOLD)));
    bar.push(Span::styled(if c.all_files { "Changed | [All files]" } else { "[Changed] | All files" }, Style::new().add_modifier(Modifier::BOLD)));
    f.render_widget(Paragraph::new(Line::from(bar)), Rect { height: 1, ..inner });
    let inner = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner };
    if c.loading && c.files.is_empty() && c.diff.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(" loading…", muted)), inner);
        return;
    }
    if let Some(e) = &c.error {
        f.render_widget(Paragraph::new(Span::styled(format!(" {e}"), Style::new().fg(Color::Red))), inner);
        return;
    }
    let shown = c.shown();
    if shown.is_empty() {
        let text = if c.all_files && c.all_loading > 0 { " listing the worktree…".to_string() } else { format!(" No changes since {}.", label.to_lowercase()) };
        f.render_widget(Paragraph::new(Span::styled(text, muted)), inner);
        return;
    }
    let list_w = (inner.width / 3).clamp(24, 48);
    let [list, sep, diff] = Layout::horizontal([Constraint::Length(list_w), Constraint::Length(1), Constraint::Min(10)]).areas(inner);
    let mut lines = Vec::new();
    for (i, (st, path, a, d)) in shown.iter().enumerate() {
        let sel = i == c.file;
        let color = match st.as_str() { "A" | "?" => Color::Green, "D" => Color::Red, "R" => Color::Cyan, _ => Color::Yellow };
        let counts = if st.is_empty() { String::new() } else { format!(" +{a} −{d}") };
        let room = (list_w as usize).saturating_sub(counts.width() + 5);
        let name = fit_path(path, room);
        let style = if sel { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else if st.is_empty() { muted } else { Style::new() };
        lines.push(Line::from(vec![
            Span::styled(if sel { "› " } else { "  " }, Style::new().fg(accent())),
            Span::styled(if st.is_empty() { "  ".to_string() } else { format!("{st} ") }, Style::new().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(name, style),
            Span::styled(counts, muted),
        ]));
    }
    let skip = c.file.saturating_sub(list.height as usize / 2).min(shown.len().saturating_sub(list.height as usize));
    f.render_widget(Paragraph::new(lines.into_iter().skip(skip).collect::<Vec<_>>()), list);
    f.render_widget(Paragraph::new(vec![Line::from(Span::styled("│", muted)); sep.height as usize]), sep);
    let w = diff.width.saturating_sub(1) as usize;
    let path = shown.get(c.file).map(|f| f.1.clone()).unwrap_or_default();
    // The file's header: which change is current and how many are accepted.
    let head = if c.unchanged {
        Line::from(vec![Span::styled(fit(&path, w.saturating_sub(24)), Style::new().add_modifier(Modifier::BOLD)), Span::styled("  unchanged · read-only", muted)])
    } else if c.hunks.is_empty() {
        Line::from(Span::styled(fit(&path, w), Style::new().add_modifier(Modifier::BOLD)))
    } else {
        Line::from(vec![
            Span::styled(fit(&path, w.saturating_sub(40)), Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  change {} of {} · {} accepted", c.change + 1, c.hunks.len(), c.accepted()), muted),
        ])
    };
    let mine = c.mine.get(&path);
    let current_at = c.hunk_at.get(c.change).copied();
    let mut body: Vec<Line> = vec![head];
    for (i, l) in c.diff.iter().enumerate().skip(c.scroll).take((diff.height as usize).saturating_sub(1)) {
        let raw = l.replace('\t', "    ");
        if l.starts_with("@@") {
            let text = fit(&raw, w.saturating_sub(24));
            let n = c.hunk_at.iter().position(|&at| at == i).unwrap_or(0);
            let current = current_at == Some(i);
            let mut spans = vec![Span::styled(if current { "› " } else { "  " }, Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::styled(text, Style::new().fg(accent()))];
            if c.hunks.get(n).is_some_and(|h| h.reviewed) {
                spans.push(Span::styled("  ✓ Accepted", Style::new().fg(Color::Green)));
            } else if current {
                spans.push(Span::styled("  a Accept · r Reject", Style::new().fg(MUTED)));
            }
            body.push(Line::from(spans));
            continue;
        }
        let yours = l.starts_with('+') && mine.is_some_and(|m| m.contains(&l[1..]));
        let text = fit(&raw, w.saturating_sub(if yours { 16 } else { 3 }));
        let style = if yours { Style::new().fg(waiting()) } else if l.starts_with('+') { Style::new().fg(Color::Green) } else if l.starts_with('-') { Style::new().fg(Color::Red) } else { Style::new() };
        let mut spans = vec![Span::raw("  "), Span::styled(text, style)];
        if yours {
            spans.push(Span::styled("  ✎ your edit", Style::new().fg(waiting())));
        }
        body.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(body), Rect { x: diff.x + 1, width: diff.width.saturating_sub(1), ..diff });
}

/// The comparisons with a key of their own.
const REVIEW_MODES: [&str; 3] = ["task_start", "latest_run", "entire_worktree"];

/// Fits a path by dropping leading directories: `…/providers/stripe/refund.ts`.
fn fit_path(path: &str, max: usize) -> String {
    if path.width() <= max {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').collect();
    for start in 1..parts.len() {
        let candidate = format!("…/{}", parts[start..].join("/"));
        if candidate.width() <= max {
            return candidate;
        }
    }
    fit(parts.last().copied().unwrap_or(path), max)
}

fn composer_height(app: &App, width: u16) -> u16 {
    let draft = app.focus.as_deref().and_then(|f| app.drafts.get(f)).map(String::as_str).unwrap_or("");
    let w = width.saturating_sub(4).max(10) as usize;
    let lines: usize = draft.split('\n').map(|l| l.width().max(1).div_ceil(w)).sum::<usize>().max(1);
    (lines as u16 + 2).min(8)
}

/// A question's lines: a long one (every file a merge commits, T-33) wraps instead of being cut.
fn confirm_height(app: &App, width: u16) -> u16 {
    let text = match &app.mode {
        Mode::Confirm(Confirm::Mods { text } | Confirm::MergePrepare { text, .. } | Confirm::MergeComplete { text, .. } | Confirm::MergeCancel { text, .. } | Confirm::Cleanup { text, .. } | Confirm::StopAll { text } | Confirm::Archive { text, .. } | Confirm::OpenPr { text, .. } | Confirm::Reject { text, .. }) => text.as_str(),
        _ => "",
    };
    ((text.width() + 8).div_ceil(width.max(20) as usize) as u16).clamp(2, 8)
}

/// Denying a permission (T-31): the note the agent gets as the reason; empty is fine.
fn deny_note(f: &mut Frame, app: &App, run: &str, area: Rect) {
    let title = short(&app.state.run(run).map(|r| r.title.clone()).unwrap_or_default(), 50);
    let what = app.feeds.get(run).and_then(|f| f.pending_permission().map(|p| p.1.to_string())).unwrap_or_default();
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(waiting()))
        .title(Line::from(vec![Span::styled(" deny → ", Style::new().fg(MUTED)), Span::styled(title, Style::new().fg(waiting()).add_modifier(Modifier::BOLD)), Span::styled(format!(" · {} ", fit(&what, 60)), Style::new().fg(MUTED))]))
        .title_bottom(Line::from(Span::styled(" a note for the agent (optional) · enter denies · esc cancels ", Style::new().fg(MUTED))).right_aligned());
    let text = if app.deny_note.is_empty() { Line::from(vec![Span::styled("▌", Style::new().fg(accent())), Span::styled(" why, or what to do instead", Style::new().fg(MUTED))]) } else { Line::from(vec![Span::raw(app.deny_note.clone()), Span::styled("▌", Style::new().fg(accent()))]) };
    f.render_widget(Paragraph::new(text).block(block.padding(ratatui::widgets::Padding::horizontal(1))), area);
}

fn composer(f: &mut Frame, app: &App, area: Rect) {
    let Some(run) = app.focused() else { return };
    let draft = app.drafts.get(&run.id).cloned().unwrap_or_default();
    let blocker = app.message_blocker(run);
    // A permission waits (AC-241): the reply denies it, and the agent reads it as the reason.
    let lead = if run.permission_request().is_some() { " deny with a note → " } else { " message → " };
    let title = Line::from(vec![Span::styled(lead, Style::new().fg(MUTED)), Span::styled(short(&run.title, 50), Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::raw(" ")]);
    let mut block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent())).title(title);
    if let Some(why) = &blocker {
        block = block.title_bottom(Line::from(Span::styled(format!(" can't send now: {why} "), Style::new().fg(waiting()))));
    }
    let mut lines: Vec<Line> = draft.split('\n').map(|l| Line::from(l.to_string())).collect();
    if let Some(last) = lines.last_mut() {
        last.spans.push(Span::styled("▌", Style::new().fg(accent())));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block.padding(ratatui::widgets::Padding::horizontal(1))), area);
}

/// Every key `?` lists (T-36: the parity table's keys are all here).
pub const HELP: &[(&str, &str)] = &[
    ("m", "Mods: library and applied guidance (Grid / Zoom)"),
    ("←↓↑→  h j k l", "move between agents"),
    ("1 – 9", "focus agent n on this page"),
    ("tab / shift+tab", "next / previous agent"),
    ("J / K", "pick the next / previous agent in the list"),
    ("esc", "close the picked agent's conversation"),
    ("L", "hide or show the agent list"),
    ("D", "dashboard mode ⇄ grid, same agent"),
    ("tab  (dashboard)", "list → review → conversation"),
    ("] [   pgdn pgup", "next / previous page"),
    ("i  enter", "message the focused agent"),
    ("g  z", "zoom: one agent's full view ⇄ grid"),
    ("home / G", "in the full view: top / bottom"),
    ("v", "review: Accept / Reject changes"),
    ("1 2 3  c  (review)", "comparison: task start, latest run, worktree"),
    ("t  (review)", "Changed | All files"),
    ("a A  r R  (review)", "Accept / Reject a change, a whole file"),
    ("e  (review)", "open the file in your $EDITOR"),
    ("F  (review)", "Follow the agent's edits (a move pauses)"),
    ("e  (in zoom)", "expand tool inputs and results"),
    ("a  s  d", "allow / deny: once, this session, with a note"),
    ("i  (waiting)", "a reply denies the permission; the agent reads it as the reason"),
    ("w", "next agent waiting for you"),
    ("x", "stop the focused agent and pause its queue"),
    ("Q", "queued messages: s send, c clear, d remove"),
    ("n", "start a new agent"),
    ("o", "Overseer: its conversation and Voice Mode"),
    ("ctrl+v  (in o)", "Voice Mode on / off"),
    ("ctrl+t  (in o)", "Voice Mode: mute / unmute"),
    ("ctrl+a  (in o)", "Voice Mode: talk to Overseer or an agent"),
    ("ctrl+x  (in o)", "Voice Mode: cancel the open request"),
    ("ctrl+y / ctrl+n", "yes / no: a read-back, a plan, a proposal"),
    ("M", "merge back (asks before each step)"),
    ("P", "open a GitHub pull request (gh)"),
    ("C", "remove a finished agent's worktree"),
    ("f", "filter: all → active → needs you → archived"),
    ("E", "archive a finished agent (Archived: restore)"),
    ("/", "search agents (esc clears)"),
    ("A", "accounts and sign-in"),
    ("O", "phone access on / off"),
    ("ctrl+o", "devices: pair a phone, revoke, scope"),
    ("S", "Audio Mode: settings and preview"),
    ("r", "reload (after X: start the daemon)"),
    ("X", "stop all agents and the daemon"),
    ("q", "quit (agents keep running)"),
];

fn help(f: &mut Frame, area: Rect) {
    let rows = HELP;
    // Two columns when one would not fit the terminal's height and two fit its width.
    let cols: usize = if rows.len() as u16 + 4 > area.height.saturating_sub(2) && area.width >= 120 { 2 } else { 1 };
    let per = rows.len().div_ceil(cols);
    let w = (72 * cols as u16).min(area.width.saturating_sub(4));
    let h = (per as u16 + 4).min(area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent())).title(Span::styled(" keys ", Style::new().add_modifier(Modifier::BOLD))).title_bottom(Line::from(Span::styled(" any key closes ", Style::new().fg(MUTED))).right_aligned());
    let inner = block.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(block, r);
    for (c, chunk) in rows.chunks(per).enumerate() {
        let mut lines = vec![Line::raw("")];
        for (k, v) in chunk {
            lines.push(Line::from(vec![Span::styled(format!("  {k:<20}"), Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::raw(*v)]));
        }
        let col_w = inner.width / cols as u16;
        f.render_widget(Paragraph::new(lines), Rect { x: inner.x + col_w * c as u16, width: col_w, ..inner });
    }
}

/// Accounts panel: each account's provider, kind and sign-in status; `s` signs in.
fn accounts(f: &mut Frame, app: &App, area: Rect) {
    let w = 96.min(area.width.saturating_sub(4));
    // Rows, plus a header and a gap per provider, plus borders and padding.
    let providers = app.accounts.iter().map(|a| a.provider.as_str()).collect::<std::collections::HashSet<_>>().len() as u16;
    let h = (app.accounts.len() as u16 + providers * 2 + 3).clamp(8, area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let mut lines = vec![Line::raw("")];
    let mut last_provider = String::new();
    for (i, a) in app.accounts.iter().enumerate() {
        if a.provider != last_provider {
            if !last_provider.is_empty() {
                lines.push(Line::raw(""));
            }
            let label = match a.provider.as_str() { "openai" => "OpenAI / ChatGPT", "anthropic" => "Anthropic / Claude", "local" => "OpenCode (local models)", p => p };
            lines.push(Line::from(Span::styled(format!("  {label}"), Style::new().fg(MUTED).add_modifier(Modifier::BOLD))));
            last_provider = a.provider.clone();
        }
        let sel = i == app.account_sel;
        let st = a.status.as_ref();
        let signed = st.and_then(|s| s["logged_in"].as_bool());
        // The plan and the shortened email say which account it is (AC-235); the fingerprint until the email is known.
        let shown = app.state.profile(&a.id).and_then(|p| p.account.clone()).unwrap_or_default();
        let plan = shown.plan.clone().or_else(|| st.and_then(|s| s["identity"]["plan"].as_str()).map(str::to_string)).unwrap_or_default();
        let fp = shown.email.clone().unwrap_or_else(|| st.and_then(|s| s["identity"]["account_fingerprint"].as_str().or(s["identity"]["fingerprint"].as_str())).map(|f| f.chars().take(8).collect::<String>()).unwrap_or_default());
        let (mark, color, text) = match signed {
            Some(true) => ("✓", Color::Green, [Some("signed in"), (!plan.is_empty()).then_some(plan.as_str()), (!fp.is_empty()).then_some(fp.as_str())].into_iter().flatten().collect::<Vec<_>>().join(" · ")),
            Some(false) => ("✗", Color::Red, "not signed in".to_string()),
            None => ("…", MUTED, "checking".to_string()),
        };
        lines.push(Line::from(vec![
            Span::styled(if sel { "  › " } else { "    " }, Style::new().fg(accent())),
            Span::styled(format!("{:<28}", fit(&crate::words::account(&a.name), 28)), if sel { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new() }),
            Span::styled(format!("{:<14}", if a.follows_app { "follows app" } else { "fixed" }), Style::new().fg(MUTED)),
            Span::styled(format!("{mark} "), Style::new().fg(color)),
            Span::styled(text, Style::new().fg(if signed == Some(true) { Color::Reset } else { color })),
        ]));
    }
    if app.accounts.is_empty() {
        lines.push(Line::from(Span::styled("  loading…", Style::new().fg(MUTED))));
    }
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent()))
        .title(Span::styled(" accounts ", Style::new().add_modifier(Modifier::BOLD)))
        .title_bottom(Line::from(Span::styled(" account login only · never API keys · s signs in · esc closes ", Style::new().fg(MUTED))).right_aligned());
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}

/// The colours of a drawn QR code: black on white whatever the terminal's own colours are
/// (fixed entries of the 256-colour cube), because a phone reads dark on light best.
pub const QR_INK: Color = Color::Indexed(16);
pub const QR_PAPER: Color = Color::Indexed(231);

/// Devices panel: paired phones with where they are and what they may do.
fn devices(f: &mut Frame, app: &App, area: Rect) {
    let p = &app.phone;
    let w = 100.min(area.width.saturating_sub(4));
    let h = (p.devices.len() as u16 + 7).clamp(9, area.height.saturating_sub(2).max(9)).min(area.height);
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let muted = Style::new().fg(MUTED);
    let mut summary = vec![Span::raw("  ")];
    summary.push(if p.enabled { Span::styled("phone access on", Style::new().fg(accent()).add_modifier(Modifier::BOLD)) } else { Span::styled("phone access off", muted.add_modifier(Modifier::BOLD)) });
    if let Some(port) = p.port.filter(|_| p.enabled) {
        summary.push(Span::styled(format!(" · port {port}"), muted));
    }
    summary.push(Span::styled(if p.notifications { " · notifications on" } else { " · notifications off" }, muted));
    if p.awake {
        summary.push(Span::styled(" · keeping this Mac awake", muted));
    }
    let mut lines = vec![Line::raw(""), Line::from(summary), Line::raw("")];
    let now = now_ms();
    let inner = w.saturating_sub(2) as usize;
    // Name, phone, presence, scope, address: the name takes what the others leave.
    let name_w = inner.saturating_sub(4 + 10 + 22 + 14 + 16).clamp(12, 34);
    for (i, d) in p.devices.iter().enumerate() {
        let sel = i == p.sel;
        let presence = d.presence(now);
        lines.push(Line::from(vec![
            Span::styled(if sel { "  › " } else { "    " }, Style::new().fg(accent())),
            Span::styled(format!("{:<name_w$}", fit(&d.name, name_w.saturating_sub(1))), if sel { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new() }),
            Span::styled(format!("{:<10}", d.platform_name()), muted),
            Span::styled(if d.connected { "● " } else { "○ " }, if d.connected { Style::new().fg(Color::Green) } else { muted }),
            Span::styled(format!("{:<20}", fit(&presence, 19)), if d.connected { Style::new() } else { muted }),
            Span::styled(format!("{:<14}", d.scope_name()), if d.scope == "watch" { Style::new().fg(waiting()) } else { Style::new() }),
            Span::styled(fit(d.address.as_deref().unwrap_or(""), 15), muted),
        ]));
    }
    if p.devices.is_empty() {
        lines.push(Line::from(Span::styled("  No phone is paired. Press p to pair one.", muted)));
    }
    if !p.waiting.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(format!("  ◆ \"{}\" asks to pair", short(&p.waiting[0].name, 40)), Style::new().fg(waiting()).add_modifier(Modifier::BOLD))));
    }
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent()))
        .title(Span::styled(" devices ", Style::new().add_modifier(Modifier::BOLD)))
        .title_bottom(Line::from(Span::styled(" switched on this Mac only · p pairs a phone · esc closes ", muted)).right_aligned());
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}

/// The QR code that fits `cols` by `lines`: level M with a full quiet zone when there is room,
/// then a narrower quiet zone, then level L (a smaller code).
fn fitting_qr(code: &str, cols: usize, lines: usize) -> Option<(qr::Qr, usize)> {
    for (level, quiet) in [(qr::Level::M, 4), (qr::Level::M, 2), (qr::Level::L, 4), (qr::Level::L, 2)] {
        let Some(size) = qr::size_for(code.len(), level) else { continue };
        let (w, h) = qr::drawn_size(size, quiet);
        if w <= cols && h <= lines {
            return qr::encode(code.as_bytes(), level).map(|q| (q, quiet));
        }
    }
    None
}

/// Words wrapped to `width` columns.
fn wrap_words(words: &[String], width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words {
        match out.last_mut() {
            Some(line) if line.width() + 1 + word.width() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => out.push(word.clone()),
        }
    }
    out
}

/// Pair a phone: the code as a QR code and as text, the time it still works, and what to do.
fn pairing(f: &mut Frame, app: &App, area: Rect) {
    let Some(p) = &app.pairing else { return };
    let muted = Style::new().fg(MUTED);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let w = 116.min(area.width.saturating_sub(2));
    let inner_w = w.saturating_sub(4) as usize;
    let max_h = area.height.saturating_sub(1).max(8);
    let inner_h = max_h.saturating_sub(2) as usize;
    let open = p.state == PairingState::Open;
    let text_w = inner_w.min(46);

    // The words beside (or under) the code.
    let mut words: Vec<Line> = vec![Line::from(Span::styled(format!("With {}", short(&p.mac, 40)), muted)), Line::raw("")];
    match &p.state {
        PairingState::Open => {
            for (n, step) in ["Open Overseer on your phone.", "Scan this code, or type it.", "Confirm the phone here, on this Mac."].iter().enumerate() {
                words.push(Line::from(vec![Span::styled(format!("{} ", n + 1), Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::raw(*step)]));
            }
            words.push(Line::raw(""));
            words.push(Line::from(Span::styled(format!("Works once, for {} more.", p.left(std::time::Instant::now())), Style::new().fg(accent()))));
            words.push(Line::raw(""));
            words.push(Line::from(Span::styled("Code to type", muted)));
            for line in wrap_words(&code_groups(&p.code), text_w) {
                words.push(Line::from(Span::styled(line, bold)));
            }
        }
        PairingState::Waiting(name) => {
            words.push(Line::from(Span::styled(format!("\"{}\" is asking to pair.", short(name, 40)), Style::new().fg(waiting()).add_modifier(Modifier::BOLD))));
            words.push(Line::from(Span::styled("Answer below: y pairs it, n does not.", muted)));
        }
        PairingState::Paired(name) => {
            words.push(Line::from(Span::styled(format!("Paired with \"{}\".", short(name, 40)), Style::new().fg(Color::Green).add_modifier(Modifier::BOLD))));
            words.push(Line::from(Span::styled("p pairs another phone · esc closes", muted)));
        }
        PairingState::Over(text) => {
            for line in wrap_words(&text.split(' ').map(str::to_string).collect::<Vec<_>>(), text_w) {
                words.push(Line::from(Span::styled(line, bold)));
            }
            words.push(Line::from(Span::styled("p makes a new code · esc closes", muted)));
        }
    }

    // Side by side when the window is wide enough for the code and the words; else stacked.
    let beside = if open { fitting_qr(&p.code, inner_w.saturating_sub(text_w + 3), inner_h.saturating_sub(1)) } else { None };
    let under = if open && beside.is_none() { fitting_qr(&p.code, inner_w, inner_h.saturating_sub(words.len() + 2)) } else { None };
    let (code, stacked) = match (beside, under) {
        (Some(c), _) => (Some(c), false),
        (None, Some(c)) => (Some(c), true),
        _ => (None, true),
    };
    if open && code.is_none() {
        words.push(Line::raw(""));
        words.push(Line::from(Span::styled("Make this window larger to see the QR code.", muted)));
    }
    let drawn: Vec<String> = code.as_ref().map(|(q, quiet)| q.half_blocks(*quiet)).unwrap_or_default();
    let code_w = drawn.first().map(|l| l.chars().count()).unwrap_or(0);
    let content_h = if stacked { drawn.len() + usize::from(!drawn.is_empty()) + words.len() } else { drawn.len().max(words.len()) };
    let content_w = if stacked { code_w.max(text_w) } else { code_w + 3 + text_w };
    let h = ((content_h + 3) as u16).min(max_h);
    let w = ((content_w + 4) as u16).clamp(40.min(w), w);
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent()))
        .title(Span::styled(" pair a phone ", bold))
        .title_bottom(Line::from(Span::styled(if p.live() { " esc cancels pairing " } else { " p new code · esc closes " }, muted)).right_aligned());
    let inner = block.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(block, r);
    let body = Rect { x: inner.x + 1, y: inner.y + 1, width: inner.width.saturating_sub(2), height: inner.height.saturating_sub(1) };
    let paper = Style::new().fg(QR_INK).bg(QR_PAPER);
    let code_lines: Vec<Line> = drawn.iter().map(|l| Line::from(Span::styled(l.clone(), paper))).collect();
    if stacked {
        let code_h = code_lines.len() as u16;
        if code_h > 0 {
            f.render_widget(Paragraph::new(code_lines), Rect { width: (code_w as u16).min(body.width), height: code_h.min(body.height), ..body });
        }
        let top = if code_h > 0 { code_h + 1 } else { 0 };
        f.render_widget(Paragraph::new(words), Rect { y: body.y + top.min(body.height), height: body.height.saturating_sub(top), ..body });
    } else {
        f.render_widget(Paragraph::new(code_lines), Rect { width: (code_w as u16).min(body.width), height: (drawn.len() as u16).min(body.height), ..body });
        let x = body.x + code_w as u16 + 3;
        f.render_widget(Paragraph::new(words), Rect { x, width: body.width.saturating_sub(code_w as u16 + 3), ..body });
    }
}

fn audio_mode(f: &mut Frame, app: &App, area: Rect) {
    let w = 76.min(area.width.saturating_sub(4));
    let h = 14.min(area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let a = &app.audio;
    let mode = if !a.known { "unknown" } else if a.enabled { "ON" } else { "OFF" };
    let signal = if a.available { "ready" } else { "unavailable" };
    let commander = if a.commander_imported { "private folder ready" } else { "no private folder" };
    let voice = if a.voice.is_empty() { "system default" } else { a.voice.as_str() };
    let lines = vec![
        Line::raw(""),
        Line::from(vec![Span::styled("  Audio Mode  ", Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::raw(format!("{mode} · {signal}"))]),
        Line::raw("  Off by default. The daemon plays one shared cue."),
        Line::raw(""),
        Line::raw(format!("  Track: {}    1 Reactor    2 System voice    3 Commander", a.track)),
        Line::raw(format!("  Voice: {voice}    v/V changes installed macOS voice")),
        Line::raw(format!("  Commander: {commander}    i imports a private folder")),
        Line::raw(""),
        Line::raw(format!("  Preview: {}    tab changes cue · p plays it", a.preview_key().replace('_', " "))),
        Line::raw(""),
        Line::raw("  space enable/disable    r refresh    esc close"),
    ];
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent()))
        .title(Span::styled(" Audio Mode ", Style::new().add_modifier(Modifier::BOLD)))
        .title_bottom(Line::from(Span::styled(" selected for every UI · played by overseerd ", Style::new().fg(MUTED))).right_aligned());
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), r);
}

fn audio_import(f: &mut Frame, app: &App, area: Rect) {
    let w = 76.min(area.width.saturating_sub(4));
    let h = 9.min(area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let lines = vec![
        Line::raw(""),
        Line::raw("  Enter the folder containing the three private Commander WAVs."),
        Line::raw("  Files stay in that folder and play in place."),
        Line::raw(""),
        Line::from(vec![Span::styled("  Path: ", Style::new().fg(accent())), Span::raw(&app.audio.import_path), Span::styled("▌", Style::new().fg(accent()))]),
        Line::raw(""),
        Line::raw("  enter import    esc back"),
    ];
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent())).title(" private Commander folder ");
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), r);
}

fn new_agent(f: &mut Frame, form: &NewAgentForm, state: &crate::model::State, area: Rect) {
    let w = 84.min(area.width.saturating_sub(4));
    let prompt_lines = form.prompt.split('\n').count().clamp(1, 6) as u16;
    let extra = [NewAgentForm::EFFORT, NewAgentForm::MODE].iter().filter(|&&f| form.has_field(f)).count() as u16;
    let h = (11 + extra + prompt_lines).min(area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let inner_w = w.saturating_sub(18) as usize;
    let generic = form.is_generic();
    let choice = |items: Vec<String>, i: usize| -> String {
        if items.is_empty() {
            return "—".into();
        }
        let cur = &items[i.min(items.len() - 1)];
        if items.len() > 1 { format!("‹ {} ›  ({}/{})", fit(cur, inner_w.saturating_sub(12)), i.min(items.len() - 1) + 1, items.len()) } else { fit(cur, inner_w) }
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let repos: Vec<String> = form.repos.iter().map(|p| if !home.is_empty() && p.starts_with(&home) { format!("~{}", &p[home.len()..]) } else { p.clone() }).collect();
    let harnesses: Vec<String> = form.harnesses.iter().map(|h| if h.2.is_empty() { crate::words::harness(&h.0) } else { format!("{} {}", crate::words::harness(&h.0), h.2) }).collect();
    let accounts: Vec<String> = form.compatible().iter().map(|&i| {
        let a = &form.accounts[i];
        let email = state.profile(&a.0).and_then(|p| p.account.as_ref()).and_then(|x| x.email.clone()).map(|e| format!(" · {e}")).unwrap_or_default();
        format!("{}{email}{}", crate::words::account(&a.1), match a.3 { Some(true) => "  ✓ signed in", Some(false) => "  ✗ not signed in", None => "" })
    }).collect();
    // T-32: effort and permission mode, only where the chosen harness takes them.
    let efforts: Vec<String> = std::iter::once("harness default".to_string()).chain(form.efforts().iter().map(|e| e.to_string())).collect();
    let modes: Vec<String> = std::iter::once("harness default".to_string()).chain(form.modes().iter().map(|m| m.1.to_string())).collect();
    let mut values: Vec<(usize, String, String)> = vec![
        (0, "Repository".into(), choice(repos, form.repo)),
        (1, "Harness".into(), choice(harnesses, form.harness)),
        if generic { (2, "Arguments".into(), form.args.clone()) } else { (2, "Account".into(), choice(accounts, form.account)) },
        if generic { (3, "Program".into(), form.program.clone()) } else { (3, "Model".into(), if form.model.is_empty() { "harness default".into() } else { form.model.clone() }) },
    ];
    if form.has_field(NewAgentForm::EFFORT) {
        values.push((NewAgentForm::EFFORT, "Effort".into(), choice(efforts, form.effort)));
    }
    if form.has_field(NewAgentForm::MODE) {
        values.push((NewAgentForm::MODE, "Permissions".into(), choice(modes, form.mode)));
    }
    values.push((NewAgentForm::PROMPT, "Prompt".into(), form.prompt.clone()));
    let mut lines = vec![Line::raw("")];
    for (i, label, value) in values.iter().map(|(i, l, v)| (*i, l, v)) {
        let active = form.field == i;
        let ls = if active { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new().fg(MUTED) };
        let vs = if active { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
        let mut first = true;
        for part in value.split('\n') {
            let label = if first { format!("  {}{:<12}", if active { "›" } else { " " }, label) } else { " ".repeat(15) };
            let mut spans = vec![Span::styled(label, ls), Span::styled(fit(part, inner_w), if value == "harness default" { Style::new().fg(MUTED) } else { vs })];
            if active && (i == 3 || i == NewAgentForm::PROMPT) || active && i == 2 && generic {
                spans.push(Span::styled("▌", Style::new().fg(accent())));
            }
            lines.push(Line::from(spans));
            first = false;
        }
        if i == 1 || i == 3 && !form.has_field(NewAgentForm::EFFORT) && !form.has_field(NewAgentForm::MODE) || i == NewAgentForm::MODE || i == NewAgentForm::EFFORT && !form.has_field(NewAgentForm::MODE) {
            lines.push(Line::raw(""));
        }
    }
    if let Some(e) = &form.error {
        lines.push(Line::from(Span::styled(format!("  {e}"), Style::new().fg(Color::Red))));
    } else if form.busy {
        lines.push(Line::from(Span::styled("  starting…", Style::new().fg(accent()))));
    }
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent())).title(Span::styled(" new agent ", Style::new().add_modifier(Modifier::BOLD))).title_bottom(Line::from(Span::styled(" enter starts · esc cancels ", Style::new().fg(MUTED))).right_aligned());
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}

// ---------------------------------------------------------------- conversation lines

/// The last `height` wrapped lines of a feed (newest at the bottom).
pub fn tail_lines(feed: &Feed, width: usize, height: usize) -> Vec<Line<'static>> {
    let mut rev: Vec<Vec<Line<'static>>> = Vec::new();
    let mut count = 0;
    for item in feed.items().rev() {
        let mut out = Vec::new();
        render_item(item, width, &mut out);
        count += out.len();
        rev.push(out);
        if count >= height {
            break;
        }
    }
    let mut lines: Vec<Line<'static>> = rev.into_iter().rev().flatten().collect();
    if lines.len() > height {
        lines.drain(..lines.len() - height);
    }
    lines
}

pub fn all_lines(feed: &Feed, width: usize, expanded: bool) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for item in feed.items() {
        render_item_ex(item, width, &mut out, expanded);
    }
    out
}

pub fn render_item(item: &Item, width: usize, out: &mut Vec<Line<'static>>) {
    render_item_ex(item, width, out, false);
}

pub fn render_item_ex(item: &Item, width: usize, out: &mut Vec<Line<'static>>, expanded: bool) {
    let muted = Style::new().fg(MUTED);
    let mut prefix: Vec<Span<'static>> = Vec::new();
    if item.child.is_some() {
        prefix.push(Span::styled("│ ", Style::new().fg(accent())));
    }
    let text = item.text.as_str();
    match &item.kind {
        Kind::User => {
            prefix.push(Span::styled("› ", Style::new().fg(accent()).add_modifier(Modifier::BOLD)));
            wrap(prefix, &[(text.to_string(), Style::new().add_modifier(Modifier::BOLD))], width, out);
        }
        Kind::Agent => markdown(prefix, text, width, out),
        Kind::Thinking => {
            prefix.push(Span::styled("∴ ", muted));
            wrap(prefix, &[(text.to_string(), muted.add_modifier(Modifier::ITALIC))], width, out);
        }
        Kind::Tool { name, status } => {
            prefix.push(Span::styled("⚙ ", muted));
            let mark = match status {
                ToolStatus::Done => (" ✓".to_string(), Style::new().fg(Color::Green)),
                ToolStatus::Failed => (" ✗".to_string(), Style::new().fg(Color::Red)),
                ToolStatus::Running => (" …".to_string(), muted),
            };
            let target = if text.is_empty() { String::new() } else { format!(" {text}") };
            // One line: truncate the target rather than wrapping it.
            let room = width.saturating_sub(prefix_width(&prefix) + name.width() + 3);
            let indent = prefix_width(&prefix);
            wrap(prefix, &[(name.clone(), Style::new()), (fit(&target, room), muted), mark], width, out);
            if expanded {
                if let Some((input, output)) = &item.detail {
                    let pad = vec![Span::styled(format!("{}│ ", " ".repeat(indent)), muted)];
                    for l in input.lines().take(8) {
                        wrap(pad.clone(), &[(l.to_string(), Style::new().fg(Color::Cyan))], width, out);
                    }
                    let lines: Vec<&str> = output.lines().collect();
                    for l in lines.iter().take(10) {
                        wrap(pad.clone(), &[(l.to_string(), muted)], width, out);
                    }
                    if lines.len() > 10 {
                        wrap(pad.clone(), &[(format!("… {} more lines", lines.len() - 10), muted.add_modifier(Modifier::ITALIC))], width, out);
                    }
                }
            }
        }
        Kind::Edit => {
            prefix.push(Span::styled("✎ ", Style::new().fg(Color::Yellow)));
            wrap(prefix, &[(text.to_string(), Style::new().fg(Color::Yellow))], width, out);
        }
        Kind::Out => wrap(prefix, &[(text.to_string(), Style::new())], width, out),
        Kind::ErrOut => wrap(prefix, &[(text.to_string(), Style::new().fg(Color::Red))], width, out),
        Kind::Permission { answer, .. } => match answer {
            None => {
                prefix.push(Span::styled("◆ ", Style::new().fg(waiting())));
                wrap(prefix, &[("wants ".to_string(), Style::new().fg(waiting())), (text.to_string(), Style::new().fg(waiting()).add_modifier(Modifier::BOLD))], width, out);
            }
            Some(true) => {
                prefix.push(Span::styled("✓ ", Style::new().fg(Color::Green)));
                wrap(prefix, &[("allowed ".to_string(), muted), (text.to_string(), muted)], width, out);
            }
            Some(false) => {
                prefix.push(Span::styled("✗ ", Style::new().fg(Color::Red)));
                wrap(prefix, &[("denied ".to_string(), muted), (text.to_string(), muted)], width, out);
            }
        },
        Kind::Error => {
            prefix.push(Span::styled("✖ ", Style::new().fg(Color::Red)));
            // A failure's words in plain words (AC-245): no raw [rate_limit] or HTTP code.
            wrap(prefix, &[(crate::words::plain(text), Style::new().fg(Color::Red))], width, out);
        }
        Kind::Child => {
            prefix.push(Span::styled("↳ ", Style::new().fg(accent())));
            wrap(prefix, &[(text.to_string(), Style::new().fg(accent()))], width, out);
        }
        Kind::TurnDone { ok } => {
            let c = if *ok { Color::Green } else { Color::Red };
            prefix.push(Span::styled("■ ", Style::new().fg(c)));
            wrap(prefix, &[(if *ok { text.to_string() } else { crate::words::plain(text) }, if *ok { muted } else { Style::new().fg(c) })], width, out);
        }
        Kind::Note => {
            prefix.push(Span::styled("· ", muted));
            wrap(prefix, &[(crate::words::plain(text), muted.add_modifier(Modifier::ITALIC))], width, out);
        }
    }
}

/// Agent Markdown, lightly: headings bold, bullets as •, code blocks in color, table rows
/// with thin rules, inline markers removed.
fn markdown(prefix: Vec<Span<'static>>, text: &str, width: usize, out: &mut Vec<Line<'static>>) {
    let indent: Vec<Span<'static>> = vec![Span::raw(" ".repeat(prefix_width(&prefix)))];
    let mut first = true;
    let mut fence = false;
    let mut blank = false;
    for raw in text.lines() {
        let p = if first { prefix.clone() } else { indent.clone() };
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            fence = !fence;
            continue;
        }
        if fence {
            wrap(p, &[(line.to_string(), Style::new().fg(Color::Cyan))], width, out);
            first = false;
            continue;
        }
        if line.trim().is_empty() {
            if !blank && !first {
                out.push(Line::raw(""));
            }
            blank = true;
            continue;
        }
        blank = false;
        let t = line.trim_start();
        if t.starts_with('|') {
            if t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                continue;
            }
            let cells: Vec<String> = t.trim_matches('|').split('|').map(|c| inline(c.trim())).collect();
            wrap(p, &[(cells.join("  │  "), Style::new())], width, out);
        } else if let Some(h) = t.strip_prefix("### ").or_else(|| t.strip_prefix("## ")).or_else(|| t.strip_prefix("# ")) {
            wrap(p, &[(inline(h), Style::new().add_modifier(Modifier::BOLD))], width, out);
        } else if let Some(b) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            let mut p = p;
            p.push(Span::raw("• "));
            wrap(p, &[(inline(b), Style::new())], width, out);
        } else {
            wrap(p, &[(inline(t), Style::new())], width, out);
        }
        first = false;
    }
}

/// Removes inline Markdown markers (**, `, [text](url) → text).
fn inline(s: &str) -> String {
    let mut out = s.replace("**", "").replace('`', "");
    while let (Some(a), Some(b)) = (out.find("]("), out.find(')')) {
        if b < a {
            break;
        }
        let Some(open) = out[..a].rfind('[') else { break };
        let label = out[open + 1..a].to_string();
        out.replace_range(open..=b, &label);
    }
    out
}

fn prefix_width(p: &[Span]) -> usize {
    p.iter().map(|s| s.content.width()).sum()
}

/// Word-wraps styled segments to `width`, the first line after `prefix`, the rest indented to it.
fn wrap(prefix: Vec<Span<'static>>, segments: &[(String, Style)], width: usize, out: &mut Vec<Line<'static>>) {
    let pw = prefix_width(&prefix);
    let avail = width.saturating_sub(pw).max(4);
    let mut lines: Vec<Vec<Span<'static>>> = vec![prefix];
    let mut col = 0usize;
    let indent = || vec![Span::raw(" ".repeat(pw))];
    for (text, style) in segments {
        for (li, src) in text.split('\n').enumerate() {
            if li > 0 {
                lines.push(indent());
                col = 0;
            }
            for word in split_keep(src) {
                let ww = word.width();
                if col + ww > avail && col > 0 {
                    lines.push(indent());
                    col = 0;
                    if word == " " {
                        continue;
                    }
                }
                if ww > avail {
                    // A word longer than the line: break it by characters.
                    let mut chunk = String::new();
                    let mut cw = 0;
                    for ch in word.chars() {
                        let w = ch.width().unwrap_or(0);
                        if col + cw + w > avail {
                            lines.last_mut().unwrap().push(Span::styled(std::mem::take(&mut chunk), *style));
                            lines.push(indent());
                            col = 0;
                            cw = 0;
                        }
                        chunk.push(ch);
                        cw += w;
                    }
                    col += cw;
                    lines.last_mut().unwrap().push(Span::styled(chunk, *style));
                } else {
                    col += ww;
                    lines.last_mut().unwrap().push(Span::styled(word.to_string(), *style));
                }
            }
        }
    }
    out.extend(lines.into_iter().map(Line::from));
}

/// Splits into words and the spaces between them (spaces kept as their own items).
fn split_keep(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    for (i, c) in s.char_indices() {
        let sp = c == ' ';
        match in_space {
            Some(prev) if prev != sp => {
                out.push(&s[start..i]);
                start = i;
            }
            _ => {}
        }
        in_space = Some(sp);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Fits text into `max` columns with an ellipsis (display width aware).
pub fn fit(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect()).collect()
    }

    #[test]
    fn wraps_to_width_and_indents_continuations() {
        let mut out = Vec::new();
        wrap(vec![Span::raw("› ")], &[("one two three four five".into(), Style::new())], 12, &mut out);
        let t = text(&out);
        assert!(t.iter().all(|l| l.width() <= 12), "{t:?}");
        assert_eq!(t[0], "› one two ");
        assert!(t[1].starts_with("  "));
    }

    #[test]
    fn long_words_break_instead_of_overflowing() {
        let mut out = Vec::new();
        wrap(vec![], &[("x".repeat(30), Style::new())], 10, &mut out);
        assert!(text(&out).iter().all(|l| l.width() <= 10));
        assert_eq!(text(&out).concat().len(), 30);
    }

    #[test]
    fn markdown_is_light() {
        let mut out = Vec::new();
        markdown(vec![], "## Done\n\n- **New:** `a.ts`\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n```ts\nlet x = 1;\n```\nSee [spec](https://x.y/z).", 80, &mut out);
        let t = text(&out);
        assert_eq!(t[0], "Done");
        assert!(t.contains(&"• New: a.ts".to_string()), "{t:?}");
        assert!(t.contains(&"A  │  B".to_string()), "{t:?}");
        assert!(t.contains(&"let x = 1;".to_string()));
        assert!(t.contains(&"See spec.".to_string()));
    }

    #[test]
    fn truecolor_and_256_color_accents() {
        set_truecolor(false);
        assert_eq!(accent(), Color::Indexed(141));
        set_truecolor(true);
        assert_eq!(accent(), Color::Rgb(155, 123, 255));
        set_truecolor(false);
    }

    #[test]
    fn fit_is_width_aware() {
        assert_eq!(fit("hello world", 8), "hello w…");
        assert_eq!(fit("short", 8), "short");
    }
}

/// Mods stays in the incumbent terminal shell; only public daemon projections are drawn.
fn mods_view(f: &mut Frame, app: &App, area: Rect) {
    let r = Rect { x: area.x+1, y: area.y, width: area.width.saturating_sub(2), height: area.height };
    let block=Block::bordered().border_style(Style::new().fg(accent())).title(" Mods ")
        .title_bottom(" Tab Applied/Library · ? help · Esc back ");
    let mut lines=app.mods.lines();
    if let Some(crate::app::ModsForm::Target { cursor })=&app.mods.form {
        for (i,run) in app.state.runs.iter().enumerate() {
            lines.push(format!("{} {}", if i==*cursor {">"} else {" "},if run.title.trim().is_empty(){"Untitled agent"}else{&run.title}));
        }
    }
    // Use the incumbent word wrapper before scrolling, so a selected field stays visible
    // even when preceding identifiers occupy multiple physical terminal lines.
    let inner=block.inner(r);
    let auto_focus=app.mods.form.is_some() || app.mods.library && app.mods.preview.is_none() && !app.mods.help;
    let mut wrapped=Vec::new(); let mut focus_line=None;
    for s in lines {
        let s=crate::app::mods_display(&s);
        if auto_focus && s.starts_with("> ") {focus_line=Some(wrapped.len());}
        for line in s.split('\n') {wrap(vec![], &[(line.to_string(),Style::new())],inner.width.max(1) as usize,&mut wrapped);}
    }
    let mut scroll=app.mods.scroll as usize;
    if let Some(at)=focus_line {if at<scroll {scroll=at;} else if at>=scroll+inner.height as usize {scroll=at.saturating_add(1).saturating_sub(inner.height as usize);}}
    f.render_widget(Clear,r);
    f.render_widget(Paragraph::new(wrapped).block(block).scroll((scroll.min(u16::MAX as usize) as u16,0)),r);
}
