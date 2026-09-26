//! Rendering: the header, the 3×3 page of agents, the composer, zoom, help and the New Agent
//! form. Terminal default colors for text (works on dark and light terminals) plus a purple
//! accent and status colors; truecolor when the terminal says so, 256 colors otherwise.

use crate::app::{short, App, Confirm, Mode, NewAgentForm, PAGE};
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
        _ => ("?", Color::Magenta),
    }
}

fn status_word(status: &str) -> &str {
    match status {
        "waiting_for_user" => "needs you",
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
    let area = f.area();
    app.size = (area.width, area.height);
    let composing = matches!(app.mode, Mode::Compose) || matches!(app.mode, Mode::Confirm(_));
    let composer_h = if matches!(app.mode, Mode::Compose) { composer_height(app, area.width) } else if composing { 1 } else { 0 };
    let [head, body, comp, foot] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(composer_h), Constraint::Length(1)]).areas(area);
    header(f, app, head);
    match app.mode {
        Mode::Zoom { .. } => zoom(f, app, body),
        _ if app.state.runs.is_empty() || app.visible().is_empty() => empty(f, app, body),
        _ if area.width < 100 || area.height < 30 => compact_layout(f, app, body),
        _ => grid(f, app, body),
    }
    if matches!(app.mode, Mode::Compose) {
        composer(f, app, comp);
    } else if let Mode::Confirm(c) = &app.mode {
        let text = match c {
            Confirm::Interrupt(id) => format!(" Interrupt {}? y / n", short(&app.state.run(id).map(|r| r.title.clone()).unwrap_or_default(), 50)),
            Confirm::Quit => " Unsent drafts will be lost. Quit? y / n".to_string(),
        };
        f.render_widget(Paragraph::new(Line::from(Span::styled(text, Style::new().fg(waiting()).add_modifier(Modifier::BOLD)))), comp);
    }
    footer(f, app, foot);
    match app.mode {
        Mode::Help => help(f, area),
        Mode::NewAgent => new_agent(f, &app.form, area),
        _ => {}
    }
}

fn header(f: &mut Frame, app: &App, area: Rect) {
    let visible = app.visible();
    let all = app.state.agents();
    let active = all.iter().filter(|r| r.active()).count();
    let needs = all.iter().filter(|r| r.needs_you()).count();
    let dot = Style::new().fg(MUTED);
    let mut spans = vec![
        Span::styled(" ◆ Overseer ", Style::new().fg(accent()).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" page {}/{} ", app.page + 1, app.pages()), Style::new().add_modifier(Modifier::BOLD)),
        Span::styled("· ", dot),
        Span::raw(format!("{} agent{}", visible.len(), if visible.len() == 1 { "" } else { "s" })),
    ];
    if active > 0 {
        spans.push(Span::styled(" · ", dot));
        spans.push(Span::styled(format!("● {active} active"), Style::new().fg(Color::Cyan)));
    }
    if needs > 0 {
        spans.push(Span::styled(" · ", dot));
        spans.push(Span::styled(format!("◆ {needs} need{} you", if needs == 1 { "s" } else { "" }), Style::new().fg(waiting()).add_modifier(Modifier::BOLD)));
    }
    if app.filter != crate::app::Filter::All {
        spans.push(Span::styled(" · ", dot));
        spans.push(Span::styled(format!("filter: {}", app.filter.label()), Style::new().fg(accent())));
    }
    let left = Line::from(spans);
    let right = if app.connected { Line::from(Span::styled("● connected ", Style::new().fg(Color::Green))) } else { Line::from(Span::styled("○ reconnecting ", Style::new().fg(Color::Red))) };
    f.render_widget(Paragraph::new(left), area);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
}

fn footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some((text, _, error)) = &app.notice {
        let style = if *error { Style::new().fg(Color::Red) } else { Style::new().fg(accent()) };
        f.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))), area);
        return;
    }
    let keys: &[(&str, &str)] = match app.mode {
        Mode::Compose => &[("enter", "send"), ("alt+enter", "new line"), ("esc", "close (keeps draft)"), ("ctrl+u", "clear")],
        Mode::Zoom { .. } => &[("j/k", "scroll"), ("g/G", "top/bottom"), ("i", "message"), ("a/d", "allow/deny"), ("x", "interrupt"), ("z", "grid"), ("?", "help")],
        Mode::NewAgent => &[("tab", "next field"), ("←/→", "choose"), ("enter", "start"), ("esc", "cancel")],
        _ => &[("←↑↓→", "move"), ("i", "message"), ("z", "zoom"), ("a/d", "allow/deny"), ("w", "next waiting"), ("]/[", "page"), ("n", "new"), ("f", "filter"), ("?", "help"), ("q", "quit")],
    };
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
        "No agents match this filter. Press f to change it."
    };
    let y = area.y + area.height / 2;
    f.render_widget(Paragraph::new(Line::from(Span::styled(msg, Style::new().fg(MUTED)))).alignment(Alignment::Center), Rect { y, height: 1, ..area });
}

fn grid(f: &mut Frame, app: &mut App, area: Rect) {
    let agents: Vec<Run> = app.page_agents().into_iter().cloned().collect();
    let rows = Layout::vertical([Constraint::Ratio(1, 3); 3]).split(area);
    for (slot, run) in agents.iter().enumerate().take(PAGE) {
        let cols = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(rows[slot / 3]);
        tile(f, app, run, slot + 1, cols[slot % 3], false);
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
    let focused = app.focus.as_deref() == Some(run.id.as_str());
    let (glyph, color) = status_mark(&run.status);
    let border = if focused { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new().fg(MUTED) };
    let account = run.profile_id.as_deref().and_then(|p| app.state.profile(p)).map(|p| p.name.replace(" (existing login)", "")).unwrap_or_default();
    let mut meta = vec![run.harness.replace("codex-app", "codex")];
    if !account.is_empty() {
        meta.push(account);
    }
    if let Some(m) = run.model.as_deref().filter(|m| !m.is_empty()) {
        meta.push(m.to_string());
    }
    let age = elapsed(run.ended_ms.unwrap_or_else(now_ms) - run.created_ms);
    let right = format!(" {} · {} ", meta.join(" · "), age);
    let room = (area.width as usize).saturating_sub(right.width() + 8);
    let title_style = if focused { Style::new().add_modifier(Modifier::BOLD).fg(accent()) } else { Style::new().add_modifier(Modifier::BOLD) };
    let title = Line::from(vec![
        Span::styled(if slot > 0 { format!(" {slot} ") } else { " ".into() }, Style::new().fg(MUTED)),
        Span::styled(format!("{glyph} "), Style::new().fg(color)),
        Span::styled(fit(&run.title, room.max(8)), title_style),
        Span::raw(" "),
    ]);
    let mut block = Block::default().borders(Borders::ALL).border_type(if focused { BorderType::Thick } else { BorderType::Rounded }).border_style(border).title(title);
    if area.width as usize > right.width() + 20 {
        block = block.title_top(Line::from(Span::styled(right, Style::new().fg(MUTED))).right_aligned());
    }
    // Bottom: what needs attention, a draft, or the status.
    let feed = app.feeds.get(&run.id);
    let pending = run.permission_request().is_some() || feed.and_then(|f| f.pending_permission()).is_some();
    let bottom = if pending {
        let what = feed.and_then(|f| f.pending_permission().map(|p| p.1.to_string())).or_else(|| run.attention.as_ref().and_then(|a| a["tool"].as_str().map(str::to_string))).unwrap_or_default();
        Some(Line::from(vec![Span::styled(" ◆ ", Style::new().fg(waiting())), Span::styled(fit(&what, (area.width as usize).saturating_sub(24)), Style::new().fg(waiting()).add_modifier(Modifier::BOLD)), Span::styled("  a", Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::styled(" allow ", Style::new().fg(MUTED)), Span::styled("d", Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::styled(" deny ", Style::new().fg(MUTED))]))
    } else if app.drafts.get(&run.id).is_some_and(|d| !d.trim().is_empty()) && !matches!(app.mode, Mode::Compose) {
        Some(Line::from(Span::styled(" ✎ draft ", Style::new().fg(accent()))))
    } else if !run.active() {
        let word = run.exit_reason.as_deref().filter(|_| run.status != "completed").map(|r| format!(" {} · {} ", status_word(&run.status), short(r, 40))).unwrap_or_else(|| format!(" {} ", status_word(&run.status)));
        Some(Line::from(Span::styled(word, Style::new().fg(color))))
    } else {
        feed.filter(|f| f.tokens_in + f.tokens_out > 0).map(|f| Line::from(Span::styled(format!(" {} in / {} out ", compact(f.tokens_in), compact(f.tokens_out)), Style::new().fg(MUTED))))
    };
    if let Some(b) = bottom {
        block = block.title_bottom(b);
    }
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    app.hit.push((run.id.clone(), area.x, area.y, area.width, area.height));
    let width = inner.width.saturating_sub(1) as usize;
    let lines = match (feed, zoomed) {
        (Some(feed), true) => {
            let all = all_lines(feed, width);
            let h = inner.height as usize;
            let max_scroll = all.len().saturating_sub(h);
            let scroll = match app.mode {
                Mode::Zoom { scroll } => scroll.min(max_scroll),
                _ => 0,
            };
            if let Mode::Zoom { scroll: s } = &mut app.mode {
                *s = scroll;
            }
            let end = all.len() - scroll;
            all[end.saturating_sub(h)..end].to_vec()
        }
        (Some(feed), false) => tail_lines(feed, width, inner.height as usize),
        (None, _) => vec![Line::from(Span::styled("loading…", Style::new().fg(MUTED)))],
    };
    let lines = if lines.is_empty() {
        let prompt = app.state.task(&run.task_id).map(|t| t.prompt.clone()).unwrap_or_default();
        let mut out = Vec::new();
        if !prompt.trim().is_empty() {
            render_item(&Item { seq: 0, kind: Kind::User, text: prompt, child: None }, width, &mut out);
        }
        out.push(Line::from(Span::styled(if run.active() { "working…" } else { "no output" }, Style::new().fg(MUTED))));
        out
    } else {
        lines
    };
    f.render_widget(Paragraph::new(lines), Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner });
}

fn composer_height(app: &App, width: u16) -> u16 {
    let draft = app.focus.as_deref().and_then(|f| app.drafts.get(f)).map(String::as_str).unwrap_or("");
    let w = width.saturating_sub(4).max(10) as usize;
    let lines: usize = draft.split('\n').map(|l| l.width().max(1).div_ceil(w)).sum::<usize>().max(1);
    (lines as u16 + 2).min(8)
}

fn composer(f: &mut Frame, app: &App, area: Rect) {
    let Some(run) = app.focused() else { return };
    let draft = app.drafts.get(&run.id).cloned().unwrap_or_default();
    let blocker = app.message_blocker(run);
    let title = Line::from(vec![Span::styled(" message → ", Style::new().fg(MUTED)), Span::styled(short(&run.title, 50), Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::raw(" ")]);
    let mut block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent())).title(title);
    if let Some(why) = &blocker {
        block = block.title_bottom(Line::from(Span::styled(format!(" can't send now: {why} "), Style::new().fg(waiting()))));
    }
    let mut lines: Vec<Line> = draft.split('\n').map(|l| Line::from(l.to_string())).collect();
    if let Some(last) = lines.last_mut() {
        last.spans.push(Span::styled("▌", Style::new().fg(accent())));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

fn help(f: &mut Frame, area: Rect) {
    let rows: &[(&str, &str)] = &[
        ("←↓↑→  h j k l", "move between agents"),
        ("1 – 9", "focus agent n on this page"),
        ("tab / shift+tab", "next / previous agent"),
        ("] [   pgdn pgup", "next / previous page"),
        ("i  enter", "message the focused agent"),
        ("z", "zoom: full screen with scrollback"),
        ("a / d", "allow / deny its permission request"),
        ("w", "next agent waiting for you"),
        ("x", "interrupt the focused agent"),
        ("n", "start a new agent"),
        ("f", "filter: all → active → needs you"),
        ("r", "reload from the daemon"),
        ("q", "quit (agents keep running)"),
    ];
    let w = 58.min(area.width.saturating_sub(4));
    let h = (rows.len() as u16 + 4).min(area.height.saturating_sub(2));
    let r = Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    let mut lines = vec![Line::raw("")];
    for (k, v) in rows {
        lines.push(Line::from(vec![Span::styled(format!("  {k:<18}"), Style::new().fg(accent()).add_modifier(Modifier::BOLD)), Span::raw(*v)]));
    }
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(accent())).title(Span::styled(" keys ", Style::new().add_modifier(Modifier::BOLD))).title_bottom(Line::from(Span::styled(" any key closes ", Style::new().fg(MUTED))).right_aligned());
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}

fn new_agent(f: &mut Frame, form: &NewAgentForm, area: Rect) {
    let w = 84.min(area.width.saturating_sub(4));
    let prompt_lines = form.prompt.split('\n').count().clamp(1, 6) as u16;
    let h = (11 + prompt_lines).min(area.height.saturating_sub(2));
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
    let harnesses: Vec<String> = form.harnesses.iter().map(|h| if h.2.is_empty() { h.0.clone() } else { format!("{} {}", h.0, h.2) }).collect();
    let accounts: Vec<String> = form.compatible().iter().map(|&i| {
        let a = &form.accounts[i];
        format!("{}{}", a.1, match a.3 { Some(true) => "  ✓ signed in", Some(false) => "  ✗ not signed in", None => "" })
    }).collect();
    let values: Vec<(String, String)> = vec![
        ("Repository".into(), choice(repos, form.repo)),
        ("Harness".into(), choice(harnesses, form.harness)),
        if generic { ("Arguments".into(), form.args.clone()) } else { ("Account".into(), choice(accounts, form.account)) },
        if generic { ("Program".into(), form.program.clone()) } else { ("Model".into(), if form.model.is_empty() { "harness default".into() } else { form.model.clone() }) },
        ("Prompt".into(), form.prompt.clone()),
    ];
    let mut lines = vec![Line::raw("")];
    for (i, (label, value)) in values.iter().enumerate() {
        let active = form.field == i;
        let ls = if active { Style::new().fg(accent()).add_modifier(Modifier::BOLD) } else { Style::new().fg(MUTED) };
        let vs = if active { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
        let mut first = true;
        for part in value.split('\n') {
            let label = if first { format!("  {}{:<12}", if active { "›" } else { " " }, label) } else { " ".repeat(15) };
            let mut spans = vec![Span::styled(label, ls), Span::styled(fit(part, inner_w), if value == "harness default" { Style::new().fg(MUTED) } else { vs })];
            if active && i >= 3 || active && i == 2 && generic {
                spans.push(Span::styled("▌", Style::new().fg(accent())));
            }
            lines.push(Line::from(spans));
            first = false;
        }
        if i == 1 || i == 3 {
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

pub fn all_lines(feed: &Feed, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for item in feed.items() {
        render_item(item, width, &mut out);
    }
    out
}

pub fn render_item(item: &Item, width: usize, out: &mut Vec<Line<'static>>) {
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
            wrap(prefix, &[(name.clone(), Style::new()), (fit(&target, room), muted), mark], width, out);
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
            wrap(prefix, &[(text.to_string(), Style::new().fg(Color::Red))], width, out);
        }
        Kind::Child => {
            prefix.push(Span::styled("↳ ", Style::new().fg(accent())));
            wrap(prefix, &[(text.to_string(), Style::new().fg(accent()))], width, out);
        }
        Kind::TurnDone { ok } => {
            let c = if *ok { Color::Green } else { Color::Red };
            prefix.push(Span::styled("■ ", Style::new().fg(c)));
            wrap(prefix, &[(text.to_string(), if *ok { muted } else { Style::new().fg(c) })], width, out);
        }
        Kind::Note => {
            prefix.push(Span::styled("· ", muted));
            wrap(prefix, &[(text.to_string(), muted.add_modifier(Modifier::ITALIC))], width, out);
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
    fn fit_is_width_aware() {
        assert_eq!(fit("hello world", 8), "hello w…");
        assert_eq!(fit("short", 8), "short");
    }
}
