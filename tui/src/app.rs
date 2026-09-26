//! The TUI's app state and behavior, independent of the terminal: key handling, pages of nine,
//! focus, the composer, answers to permissions, new agents, and the replies and events coming
//! from the daemon. `ui::draw` renders it; tests drive it directly.

use crate::client::{Msg, Requests};
use crate::feed::Feed;
use crate::model::{Run, State};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const PAGE: usize = 9;
/// Pages of history fetched per run (5,000 events each), newest kept by the feed cap.
const HISTORY_PAGES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Active,
    NeedsYou,
}

impl Filter {
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Active => "active",
            Filter::NeedsYou => "needs you",
        }
    }
    fn next(self) -> Filter {
        match self {
            Filter::All => Filter::Active,
            Filter::Active => Filter::NeedsYou,
            Filter::NeedsYou => Filter::All,
        }
    }
    fn keeps(self, r: &Run) -> bool {
        match self {
            Filter::All => true,
            Filter::Active => r.active(),
            Filter::NeedsYou => r.needs_you(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Grid,
    /// One agent full screen; `scroll` counts lines up from the bottom (0 = following).
    Zoom { scroll: usize },
    Help,
    /// Typing a message to the focused agent (drafts live in `drafts`).
    Compose,
    Confirm(Confirm),
    NewAgent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    Interrupt(String),
    Quit,
}

/// What a pending request was for.
#[derive(Debug, Clone)]
enum Pending {
    State,
    History { root: String, run: String, page: usize },
    FollowUp { run: String, text: String },
    Permission { allow: bool },
    Interrupt,
    Harnesses,
    Accounts,
    ProfileStatus(String),
    Create,
}

/// The New Agent form.
#[derive(Debug, Clone, Default)]
pub struct NewAgentForm {
    pub field: usize,
    pub repos: Vec<String>,
    pub repo: usize,
    pub harnesses: Vec<(String, bool, String)>,
    pub harness: usize,
    /// (id, name, harnesses it can run, signed in)
    pub accounts: Vec<(String, String, Vec<String>, Option<bool>)>,
    pub account: usize,
    pub model: String,
    pub prompt: String,
    pub program: String,
    pub args: String,
    pub error: Option<String>,
    pub busy: bool,
}

impl NewAgentForm {
    pub const FIELDS: [&'static str; 5] = ["Repository", "Harness", "Account", "Model", "Prompt"];

    pub fn harness_id(&self) -> Option<&str> {
        self.harnesses.get(self.harness).map(|h| h.0.as_str())
    }

    /// Accounts compatible with the chosen harness, signed-in ones first.
    pub fn compatible(&self) -> Vec<usize> {
        let h = self.harness_id().unwrap_or_default();
        let mut idx: Vec<usize> = (0..self.accounts.len()).filter(|&i| self.accounts[i].2.iter().any(|x| x == h)).collect();
        idx.sort_by_key(|&i| match self.accounts[i].3 {
            Some(true) => 0,
            None => 1,
            Some(false) => 2,
        });
        idx
    }

    pub fn is_generic(&self) -> bool {
        self.harness_id() == Some("generic")
    }
}

pub struct App {
    client: Arc<dyn Requests>,
    pub state: State,
    pub feeds: HashMap<String, Feed>,
    /// The focused agent (a top-level run id); focus follows the agent, not the slot.
    pub focus: Option<String>,
    pub page: usize,
    pub filter: Filter,
    pub mode: Mode,
    pub drafts: HashMap<String, String>,
    pub form: NewAgentForm,
    pub notice: Option<(String, Instant, bool)>,
    pub connected: bool,
    pub quit: bool,
    /// Something changed since the last draw.
    pub dirty: bool,
    /// Terminal size in cells (set by the event loop; used for layout decisions and mouse hits).
    pub size: (u16, u16),
    /// Tile rectangles from the last draw: (run id, x, y, w, h), for mouse clicks.
    pub hit: Vec<(String, u16, u16, u16, u16)>,
    /// Working directory's Git root (the default repository for new agents).
    pub cwd_repo: Option<String>,
    pending: HashMap<u64, Pending>,
    history_requested: HashSet<String>,
    state_due: Option<Instant>,
    state_inflight: bool,
    subscribed_generation: u64,
    connect_generation: u64,
    last_repo: Option<String>,
    /// Time of the last event per agent (for the "just now" activity marker).
    pub last_event: HashMap<String, Instant>,
    /// Timestamps for latency tests: when a key was handled and when an event arrived.
    pub stats: Stats,
    /// The composer was opened from zoom (Esc returns there).
    zoom_return: bool,
    /// Typing a new repository path in the New Agent form.
    editing_repo: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub draws: u64,
    pub events: u64,
}

impl App {
    pub fn new(client: Arc<dyn Requests>) -> App {
        App {
            client,
            state: State::default(),
            feeds: HashMap::new(),
            focus: None,
            page: 0,
            filter: Filter::All,
            mode: Mode::Grid,
            drafts: HashMap::new(),
            form: NewAgentForm::default(),
            notice: None,
            connected: false,
            quit: false,
            dirty: true,
            size: (120, 40),
            hit: Vec::new(),
            cwd_repo: None,
            pending: HashMap::new(),
            history_requested: HashSet::new(),
            state_due: None,
            state_inflight: false,
            subscribed_generation: 0,
            connect_generation: 0,
            last_repo: None,
            last_event: HashMap::new(),
            stats: Stats::default(),
            zoom_return: false,
            editing_repo: false,
        }
    }

    fn request(&mut self, method: &str, params: Value, why: Pending) {
        let id = self.client.request(method, params);
        self.pending.insert(id, why);
    }

    fn say(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Some((text.into(), Instant::now(), error));
        self.dirty = true;
    }

    // ---------------------------------------------------------------- agents and pages

    /// Agents shown with the current filter, newest first.
    pub fn visible(&self) -> Vec<&Run> {
        self.state.agents().into_iter().filter(|r| self.filter.keeps(r)).collect()
    }

    pub fn pages(&self) -> usize {
        self.visible().len().div_ceil(PAGE).max(1)
    }

    /// The agents on the current page (at most nine).
    pub fn page_agents(&self) -> Vec<&Run> {
        self.visible().into_iter().skip(self.page * PAGE).take(PAGE).collect()
    }

    pub fn focused(&self) -> Option<&Run> {
        self.focus.as_deref().and_then(|id| self.state.run(id))
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.visible().iter().position(|r| r.id == id)
    }

    fn focus_index(&mut self, i: usize) {
        let ids: Vec<String> = self.visible().iter().map(|r| r.id.clone()).collect();
        if ids.is_empty() {
            self.focus = None;
            self.page = 0;
            return;
        }
        let i = i.min(ids.len() - 1);
        self.focus = Some(ids[i].clone());
        self.page = i / PAGE;
        self.dirty = true;
        self.ensure_history();
    }

    /// Keeps focus on the same agent after the list changed (new agents, filter changes).
    fn settle_focus(&mut self) {
        let n = self.visible().len();
        if n == 0 {
            self.focus = None;
            self.page = 0;
            return;
        }
        match self.focus.clone().and_then(|f| self.index_of(&f)) {
            Some(i) => self.page = i / PAGE,
            None => {
                let i = (self.page * PAGE).min(n - 1);
                self.focus_index(i);
            }
        }
        self.page = self.page.min(self.pages() - 1);
    }

    fn move_focus(&mut self, dx: i32, dy: i32) {
        let Some(i) = self.focus.clone().and_then(|f| self.index_of(&f)) else {
            self.focus_index(self.page * PAGE);
            return;
        };
        let n = self.visible().len();
        let (page, slot) = (i / PAGE, i % PAGE);
        let (row, col) = ((slot / 3) as i32, (slot % 3) as i32);
        let target = if dx != 0 {
            let c = col + dx;
            if c < 0 {
                // Past the left edge: the previous page's same row, rightmost column.
                if page == 0 { None } else { Some((page - 1) * PAGE + (row as usize) * 3 + 2) }
            } else if c > 2 {
                Some((page + 1) * PAGE + (row as usize) * 3)
            } else {
                Some(page * PAGE + (row * 3 + c) as usize)
            }
        } else {
            let r = row + dy;
            if (0..3).contains(&r) { Some(page * PAGE + (r * 3 + col) as usize) } else { None }
        };
        if let Some(t) = target {
            if t < n {
                self.focus_index(t);
            } else if dx > 0 && t >= n && (page + 1) * PAGE < n {
                self.focus_index(n - 1);
            }
        }
    }

    fn change_page(&mut self, delta: i32) {
        let pages = self.pages() as i32;
        let next = (self.page as i32 + delta).clamp(0, pages - 1) as usize;
        if next == self.page {
            return;
        }
        let slot = self.focus.clone().and_then(|f| self.index_of(&f)).map(|i| i % PAGE).unwrap_or(0);
        let n = self.visible().len();
        self.focus_index((next * PAGE + slot).min(n.saturating_sub(1)));
    }

    /// Loads history for the agents on screen (and the zoomed one) that have none yet.
    pub fn ensure_history(&mut self) {
        let mut want: Vec<String> = self.page_agents().iter().map(|r| r.id.clone()).collect();
        if let Some(f) = &self.focus {
            want.push(f.clone());
        }
        for root in want {
            if self.history_requested.contains(&root) || !self.connected {
                continue;
            }
            self.history_requested.insert(root.clone());
            let mut runs = vec![root.clone()];
            runs.extend(self.state.descendants(&root).iter().map(|r| r.id.clone()));
            for run in runs {
                self.request("events.list", json!({ "run_id": run, "after": 0, "limit": 5000 }), Pending::History { root: root.clone(), run, page: 0 });
            }
        }
    }

    // ---------------------------------------------------------------- daemon messages

    pub fn handle_msg(&mut self, msg: Msg) {
        self.dirty = true;
        match msg {
            Msg::Connected => {
                self.connected = true;
                self.connect_generation += 1;
                self.state_inflight = false;
                self.request_state();
            }
            Msg::Disconnected(why) => {
                self.connected = false;
                // Replies to requests on the old connection never come.
                self.pending.clear();
                self.state_inflight = false;
                self.history_requested.retain(|r| self.feeds.get(r).is_some_and(|f| f.history_loaded));
                self.say(format!("Reconnecting to overseerd ({why})…"), true);
            }
            Msg::Replayed => {}
            Msg::Event(ev) => self.on_event(ev),
            Msg::Reply { id, result } => {
                if let Some(why) = self.pending.remove(&id) {
                    self.on_reply(why, result);
                }
            }
        }
    }

    fn request_state(&mut self) {
        if self.state_inflight {
            self.state_due = Some(Instant::now() + Duration::from_millis(120));
            return;
        }
        self.state_inflight = true;
        self.state_due = None;
        self.request("state", json!({}), Pending::State);
    }

    /// Timers: the debounced state reload. Returns true when something changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if let Some(due) = self.state_due {
            if now >= due && self.connected {
                self.request_state();
            }
        }
        if let Some((_, at, _)) = &self.notice {
            if now.duration_since(*at) > Duration::from_secs(6) {
                self.notice = None;
                changed = true;
            }
        }
        changed
    }

    fn on_event(&mut self, ev: Value) {
        self.stats.events += 1;
        let run_id = ev["run_id"].as_str().unwrap_or_default().to_string();
        let kind = ev["kind"].as_str().unwrap_or_default().to_string();
        if !run_id.is_empty() {
            let root = self.state.root_of(&run_id);
            let child = if root != run_id { Some(self.state.run(&run_id).map(|r| r.title.clone()).unwrap_or_else(|| "sub-agent".into())) } else { None };
            let ws_path = self.state.run(&root).and_then(|r| self.state.workspace(&r.workspace_id)).map(|w| w.path.clone());
            let feed = self.feeds.entry(root.clone()).or_default();
            if feed.root.is_none() {
                feed.root = ws_path;
            }
            feed.add(&ev, child.as_deref());
            self.last_event.insert(root, Instant::now());
        }
        // Statuses, turns and new runs come from `state`, reloaded like VS Code does.
        if matches!(kind.as_str(), "status" | "turn_started" | "turn_done" | "permission" | "permission_answered" | "child" | "child_reparented" | "task_created" | "workspace_removed" | "reattached")
            || (!run_id.is_empty() && self.state.run(&run_id).is_none())
        {
            if self.state_due.is_none() {
                self.state_due = Some(Instant::now() + Duration::from_millis(80));
            }
        }
    }

    fn on_reply(&mut self, why: Pending, result: Result<Value, String>) {
        match (why, result) {
            (Pending::State, Ok(v)) => {
                self.state_inflight = false;
                match serde_json::from_value::<State>(v) {
                    Ok(state) => {
                        let cursor = state.cursor;
                        self.state = state;
                        for (root, feed) in self.feeds.iter_mut() {
                            if feed.root.is_none() {
                                feed.root = self.state.run(root).and_then(|r| self.state.workspace(&r.workspace_id)).map(|w| w.path.clone());
                            }
                        }
                        if self.subscribed_generation != self.connect_generation {
                            self.subscribed_generation = self.connect_generation;
                            self.client.set_cursor_if_unset(cursor);
                            self.client.subscribe();
                        }
                        self.settle_focus();
                        self.ensure_history();
                    }
                    Err(e) => self.say(format!("Could not read the daemon state: {e}"), true),
                }
                if self.state_due.is_some_and(|d| d <= Instant::now()) {
                    self.request_state();
                }
            }
            (Pending::State, Err(e)) => {
                self.state_inflight = false;
                self.say(format!("state: {e}"), true);
            }
            (Pending::History { root, run, page }, Ok(v)) => {
                let events = v["events"].as_array().cloned().unwrap_or_default();
                let child = if root != run { Some(self.state.run(&run).map(|r| r.title.clone()).unwrap_or_else(|| "sub-agent".into())) } else { None };
                let ws = self.state.run(&root).and_then(|r| self.state.workspace(&r.workspace_id)).map(|w| w.path.clone());
                let feed = self.feeds.entry(root.clone()).or_default();
                if feed.root.is_none() {
                    feed.root = ws;
                }
                for e in &events {
                    feed.add(e, child.as_deref());
                }
                if events.len() == 5000 && page + 1 < HISTORY_PAGES {
                    let after = events.last().and_then(|e| e["seq"].as_i64()).unwrap_or(0);
                    self.request("events.list", json!({ "run_id": run, "after": after, "limit": 5000 }), Pending::History { root, run, page: page + 1 });
                } else if run == root {
                    feed.history_loaded = true;
                }
            }
            (Pending::History { root, .. }, Err(e)) => {
                self.history_requested.remove(&root);
                self.say(format!("history: {e}"), true);
            }
            (Pending::FollowUp { run, .. }, Ok(_)) => {
                let title = self.state.run(&run).map(|r| r.title.clone()).unwrap_or_default();
                self.say(format!("Sent to {}", short(&title, 40)), false);
                self.state_due = Some(Instant::now() + Duration::from_millis(60));
            }
            (Pending::FollowUp { run, text }, Err(e)) => {
                // Keep what was typed.
                let d = self.drafts.entry(run).or_default();
                if d.is_empty() {
                    *d = text;
                }
                self.say(format!("Not sent: {e}"), true);
            }
            (Pending::Permission { allow }, Ok(_)) => {
                self.say(if allow { "Allowed" } else { "Denied" }, false);
                self.state_due = Some(Instant::now() + Duration::from_millis(60));
            }
            (Pending::Interrupt, Ok(_)) => self.say("Interrupt sent", false),
            (Pending::Harnesses, Ok(v)) => {
                let list = v.as_array().cloned().unwrap_or_default();
                self.form.harnesses = list.iter().map(|h| (h["harness"].as_str().unwrap_or_default().to_string(), h["installed"].as_bool().unwrap_or(false), h["version"].as_str().unwrap_or_default().to_string())).filter(|h| h.1).collect();
                // Claude Code first, then Codex, then the rest.
                let rank = |h: &str| match h { "claude" => 0, "codex" => 1, "codex-app" => 2, "opencode" => 3, _ => 4 };
                self.form.harnesses.sort_by_key(|h| rank(&h.0));
            }
            (Pending::Accounts, Ok(v)) => {
                let list = v["accounts"].as_array().cloned().unwrap_or_default();
                self.form.accounts = list.iter().map(|a| (a["id"].as_str().unwrap_or_default().to_string(), a["name"].as_str().unwrap_or_default().to_string(), a["harnesses"].as_array().map(|x| x.iter().filter_map(|h| h.as_str().map(str::to_string)).collect()).unwrap_or_default(), None)).collect();
                let ids: Vec<String> = self.form.accounts.iter().map(|a| a.0.clone()).collect();
                for id in ids {
                    self.request("profile.status", json!({ "id": id }), Pending::ProfileStatus(id.clone()));
                }
            }
            (Pending::ProfileStatus(id), Ok(v)) => {
                if let Some(a) = self.form.accounts.iter_mut().find(|a| a.0 == id) {
                    a.3 = Some(v["logged_in"].as_bool().unwrap_or(false));
                }
            }
            (Pending::Create, Ok(v)) => {
                self.form.busy = false;
                if let Some(err) = v["launch_error"].as_str() {
                    self.form.error = Some(format!("Could not launch: {err}"));
                    return;
                }
                let id = v["run"]["id"].as_str().unwrap_or_default().to_string();
                self.mode = Mode::Grid;
                self.filter = Filter::All;
                self.focus = Some(id);
                self.page = 0;
                self.form.prompt.clear();
                self.say("Started", false);
                self.request_state();
            }
            (Pending::Create, Err(e)) => {
                self.form.busy = false;
                self.form.error = Some(e);
            }
            (_, Err(e)) => self.say(e, true),
        }
    }

    // ---------------------------------------------------------------- actions

    /// Why the focused agent cannot take a message right now (None: it can).
    pub fn message_blocker(&self, run: &Run) -> Option<String> {
        if run.parent_run_id.is_some() {
            return Some("native children take messages through their parent".into());
        }
        let fu = run.capability("follow_up");
        if fu.starts_with("unsupported") {
            return Some(format!("{} does not take follow-ups", run.harness));
        }
        if run.active() && run.harness != "generic" {
            return Some("a turn is running; wait for it or press x to interrupt".into());
        }
        None
    }

    fn send_draft(&mut self) {
        let Some(run) = self.focused().cloned() else { return };
        let text = self.drafts.get(&run.id).cloned().unwrap_or_default();
        if text.trim().is_empty() {
            return;
        }
        if let Some(why) = self.message_blocker(&run) {
            self.say(format!("Not sent: {why}"), true);
            return;
        }
        self.drafts.remove(&run.id);
        self.request("run.follow_up", json!({ "run_id": run.id, "prompt": text }), Pending::FollowUp { run: run.id.clone(), text: text.clone() });
        self.mode = if matches!(self.mode, Mode::Compose) { Mode::Grid } else { self.mode.clone() };
    }

    fn answer(&mut self, allow: bool) {
        let Some(run) = self.focused().cloned() else { return };
        let request_id = run.permission_request().or_else(|| self.feeds.get(&run.id).and_then(|f| f.pending_permission().map(|p| p.0.to_string())));
        let Some(request_id) = request_id else {
            self.say("Nothing to answer for this agent", false);
            return;
        };
        self.request("run.permission", json!({ "run_id": run.id, "request_id": request_id, "allow": allow }), Pending::Permission { allow });
    }

    fn next_waiting(&mut self) {
        let list = self.visible();
        let start = self.focus.as_deref().and_then(|f| list.iter().position(|r| r.id == f)).map(|i| i + 1).unwrap_or(0);
        let n = list.len();
        let found = (0..n).map(|k| (start + k) % n).find(|&i| list[i].needs_you());
        match found {
            Some(i) => self.focus_index(i),
            None => self.say("No agent is waiting for you", false),
        }
    }

    fn open_new_agent(&mut self) {
        let mut repos: Vec<String> = Vec::new();
        for r in self.cwd_repo.iter().chain(self.last_repo.iter()) {
            if !repos.contains(r) {
                repos.push(r.clone());
            }
        }
        let mut tasks = self.state.tasks.clone();
        tasks.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
        for t in tasks {
            if !repos.contains(&t.repo_root) {
                repos.push(t.repo_root);
            }
        }
        let keep = std::mem::take(&mut self.form);
        self.form = NewAgentForm { repos, model: keep.model, program: keep.program, args: if keep.args.is_empty() { "[]".into() } else { keep.args }, harnesses: keep.harnesses, accounts: keep.accounts, field: 4, ..Default::default() };
        self.request("harness.list", json!({}), Pending::Harnesses);
        self.request("account.list", json!({}), Pending::Accounts);
        self.mode = Mode::NewAgent;
    }

    fn launch(&mut self) {
        let f = &self.form;
        let Some(repo) = f.repos.get(f.repo).cloned() else {
            self.form.error = Some("No repository: start overseer-tui inside a Git repository, or type a path.".into());
            return;
        };
        let Some(harness) = f.harness_id().map(str::to_string) else {
            self.form.error = Some("No installed harness found.".into());
            return;
        };
        let mut params = json!({ "repo": repo, "harness": harness, "workspace_mode": "worktree", "prompt": f.prompt, "title": title_of(&f.prompt) });
        if f.is_generic() {
            let args: Vec<String> = match serde_json::from_str(if f.args.trim().is_empty() { "[]" } else { &f.args }) {
                Ok(a) => a,
                Err(_) => {
                    self.form.error = Some("Arguments must be a JSON array of strings.".into());
                    return;
                }
            };
            if !f.program.starts_with('/') {
                self.form.error = Some("Program must be an absolute path.".into());
                return;
            }
            params["program"] = json!(f.program);
            params["args"] = json!(args);
            if f.prompt.trim().is_empty() {
                params["title"] = json!(title_of(&f.program));
            }
        } else {
            if f.prompt.trim().is_empty() {
                self.form.error = Some("Type what the agent should do.".into());
                return;
            }
            let compatible = f.compatible();
            let Some(&ai) = compatible.get(f.account.min(compatible.len().saturating_sub(1))) else {
                self.form.error = Some(format!("No account for {harness}. Add one in VS Code (Accounts) first."));
                return;
            };
            params["profile_id"] = json!(f.accounts[ai].0);
            if !f.model.trim().is_empty() {
                params["model"] = json!(f.model.trim());
            }
        }
        self.last_repo = Some(repo);
        self.form.error = None;
        self.form.busy = true;
        self.request("task.create", params, Pending::Create);
    }

    // ---------------------------------------------------------------- input

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if let MouseEventKind::Down(MouseButton::Left) = m.kind {
            if let Some((id, ..)) = self.hit.iter().find(|(_, x, y, w, h)| m.column >= *x && m.column < x + w && m.row >= *y && m.row < y + h).cloned() {
                if let Some(i) = self.index_of(&id) {
                    self.focus_index(i);
                }
            }
        } else if matches!(self.mode, Mode::Zoom { .. }) {
            match m.kind {
                MouseEventKind::ScrollUp => self.scroll(3),
                MouseEventKind::ScrollDown => self.scroll(-3),
                _ => {}
            }
        }
    }

    fn scroll(&mut self, delta: i64) {
        if let Mode::Zoom { scroll } = &mut self.mode {
            *scroll = (*scroll as i64 + delta).max(0) as usize;
            self.dirty = true;
        }
    }

    pub fn handle_key(&mut self, k: KeyEvent) {
        self.dirty = true;
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.try_quit();
            return;
        }
        match self.mode.clone() {
            Mode::Help => self.mode = Mode::Grid,
            Mode::Confirm(c) => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.mode = Mode::Grid;
                    match c {
                        Confirm::Interrupt(run) => self.request("run.interrupt", json!({ "run_id": run }), Pending::Interrupt),
                        Confirm::Quit => self.quit = true,
                    }
                }
                _ => self.mode = Mode::Grid,
            },
            Mode::Compose => self.compose_key(k),
            Mode::NewAgent => self.form_key(k),
            Mode::Grid | Mode::Zoom { .. } => self.nav_key(k),
        }
    }

    fn try_quit(&mut self) {
        if self.drafts.values().any(|d| !d.trim().is_empty()) && self.mode != Mode::Confirm(Confirm::Quit) {
            self.mode = Mode::Confirm(Confirm::Quit);
        } else {
            self.quit = true;
        }
    }

    fn nav_key(&mut self, k: KeyEvent) {
        let zoom = matches!(self.mode, Mode::Zoom { .. });
        match k.code {
            KeyCode::Char('q') => self.try_quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Esc if zoom => self.mode = Mode::Grid,
            KeyCode::Char('z') => self.mode = if zoom { Mode::Grid } else { Mode::Zoom { scroll: 0 } },
            KeyCode::Char('i') | KeyCode::Enter => {
                if let Some(run) = self.focused().cloned() {
                    self.mode = Mode::Compose;
                    if let Some(why) = self.message_blocker(&run) {
                        self.say(format!("{} can't take a message now: {why}", short(&run.title, 30)), false);
                    }
                    self.zoom_return = zoom;
                }
            }
            KeyCode::Char('a') => self.answer(true),
            KeyCode::Char('d') => self.answer(false),
            KeyCode::Char('w') => self.next_waiting(),
            KeyCode::Char('x') => {
                if let Some(run) = self.focused() {
                    if run.active() {
                        self.mode = Mode::Confirm(Confirm::Interrupt(run.id.clone()));
                    } else {
                        self.say("This agent is not running", false);
                    }
                }
            }
            KeyCode::Char('n') => self.open_new_agent(),
            KeyCode::Char('f') => {
                self.filter = self.filter.next();
                self.page = 0;
                self.settle_focus();
                self.ensure_history();
            }
            KeyCode::Char('r') => self.request_state(),
            // Zoom scrolling.
            KeyCode::Char('k') | KeyCode::Up if zoom => self.scroll(1),
            KeyCode::Char('j') | KeyCode::Down if zoom => self.scroll(-1),
            KeyCode::PageUp if zoom => self.scroll(self.size.1 as i64 - 4),
            KeyCode::PageDown if zoom => self.scroll(-(self.size.1 as i64 - 4)),
            KeyCode::Char('g') if zoom => self.mode = Mode::Zoom { scroll: usize::MAX / 2 },
            KeyCode::Char('G') if zoom => self.mode = Mode::Zoom { scroll: 0 },
            // Grid navigation.
            KeyCode::Left | KeyCode::Char('h') => self.move_focus(-1, 0),
            KeyCode::Right | KeyCode::Char('l') => self.move_focus(1, 0),
            KeyCode::Up | KeyCode::Char('k') => self.move_focus(0, -1),
            KeyCode::Down | KeyCode::Char('j') => self.move_focus(0, 1),
            KeyCode::Tab => self.step(1),
            KeyCode::BackTab => self.step(-1),
            KeyCode::Char(']') | KeyCode::PageDown => self.change_page(1),
            KeyCode::Char('[') | KeyCode::PageUp => self.change_page(-1),
            KeyCode::Char(c @ '1'..='9') => {
                let slot = c as usize - '1' as usize;
                if self.page * PAGE + slot < self.visible().len() {
                    self.focus_index(self.page * PAGE + slot);
                }
            }
            _ => self.dirty = false,
        }
    }

    fn step(&mut self, delta: i32) {
        let n = self.visible().len() as i32;
        if n == 0 {
            return;
        }
        let i = self.focus.clone().and_then(|f| self.index_of(&f)).map(|i| i as i32).unwrap_or(-1);
        self.focus_index(((i + delta).rem_euclid(n)) as usize);
    }

    fn compose_key(&mut self, k: KeyEvent) {
        let Some(id) = self.focus.clone() else {
            self.mode = Mode::Grid;
            return;
        };
        let back = if self.zoom_return { Mode::Zoom { scroll: 0 } } else { Mode::Grid };
        match k.code {
            KeyCode::Esc => self.mode = back,
            KeyCode::Enter if k.modifiers.contains(KeyModifiers::ALT) || k.modifiers.contains(KeyModifiers::SHIFT) => self.drafts.entry(id).or_default().push('\n'),
            KeyCode::Enter => {
                self.send_draft();
                if self.mode == Mode::Grid {
                    self.mode = back;
                }
            }
            KeyCode::Backspace => {
                self.drafts.entry(id).or_default().pop();
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.drafts.remove(&id);
            }
            KeyCode::Char('w') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                let d = self.drafts.entry(id).or_default();
                let trimmed = d.trim_end().len();
                d.truncate(trimmed);
                let cut = d.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
                d.truncate(cut);
            }
            KeyCode::Char(c) => self.drafts.entry(id).or_default().push(c),
            _ => {}
        }
    }

    /// Text typed from a paste (bracketed paste) into the composer or the form.
    pub fn paste(&mut self, text: &str) {
        self.dirty = true;
        match self.mode {
            Mode::Compose => {
                if let Some(id) = self.focus.clone() {
                    self.drafts.entry(id).or_default().push_str(text);
                }
            }
            Mode::NewAgent => {
                if let Some(s) = self.form_text() {
                    s.push_str(text);
                }
            }
            _ => {}
        }
    }

    fn form_text(&mut self) -> Option<&mut String> {
        let generic = self.form.is_generic();
        match (self.form.field, generic) {
            (0, _) => None,
            (3, false) => Some(&mut self.form.model),
            (3, true) => Some(&mut self.form.program),
            (2, true) => Some(&mut self.form.args),
            (4, _) => Some(&mut self.form.prompt),
            _ => None,
        }
    }

    fn form_key(&mut self, k: KeyEvent) {
        if self.form.busy {
            return;
        }
        let fields = NewAgentForm::FIELDS.len();
        match k.code {
            KeyCode::Esc => self.mode = Mode::Grid,
            KeyCode::Tab | KeyCode::Down => self.form.field = (self.form.field + 1) % fields,
            KeyCode::BackTab | KeyCode::Up => self.form.field = (self.form.field + fields - 1) % fields,
            KeyCode::Enter if k.modifiers.contains(KeyModifiers::ALT) || k.modifiers.contains(KeyModifiers::SHIFT) => {
                if self.form.field == 4 {
                    self.form.prompt.push('\n');
                }
            }
            KeyCode::Enter => self.launch(),
            KeyCode::Left | KeyCode::Right => {
                let d: i64 = if k.code == KeyCode::Left { -1 } else { 1 };
                let generic = self.form.is_generic();
                let cycle = |i: usize, n: usize| if n == 0 { 0 } else { ((i as i64 + d).rem_euclid(n as i64)) as usize };
                match (self.form.field, generic) {
                    (0, _) => self.form.repo = cycle(self.form.repo, self.form.repos.len()),
                    (1, _) => {
                        self.form.harness = cycle(self.form.harness, self.form.harnesses.len());
                        self.form.account = 0;
                    }
                    (2, false) => self.form.account = cycle(self.form.account, self.form.compatible().len()),
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                if self.form.field == 0 {
                    if self.editing_repo {
                        if let Some(r) = self.form.repos.get_mut(self.form.repo) {
                            r.pop();
                        }
                    }
                    return;
                }
                if let Some(s) = self.form_text() {
                    s.pop();
                }
            }
            KeyCode::Char(c) => {
                if self.form.field == 0 && c == '/' || self.form.field == 0 && c == '~' {
                    // Typing a path adds it as a repository.
                    self.form.repos.insert(0, c.to_string());
                    self.form.repo = 0;
                    self.form.field = 0;
                    self.editing_repo = true;
                    return;
                }
                if self.form.field == 0 && self.editing_repo {
                    if let Some(r) = self.form.repos.get_mut(self.form.repo) {
                        r.push(c);
                    }
                    return;
                }
                if let Some(s) = self.form_text() {
                    s.push(c);
                }
            }
            _ => {}
        }
        if self.form.field != 0 {
            self.editing_repo = false;
        }
    }
}

/// A task title from a prompt: its first line, at most 60 characters.
pub fn title_of(prompt: &str) -> String {
    let line = prompt.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("task");
    short(line, 60)
}

/// Shortens to `max` characters with an ellipsis.
pub fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
