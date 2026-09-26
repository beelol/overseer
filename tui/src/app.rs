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
    /// What the focused agent changed: files and their diff against a comparison base.
    Changes,
    /// Typing a search (`/`): agents filter as you type.
    Search,
    /// Accounts and their sign-in status (`A`).
    Accounts,
}

/// A program to run in the terminal with the TUI suspended (a provider's own sign-in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exec {
    pub title: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// One row of the Accounts panel.
#[derive(Debug, Clone, Default)]
pub struct AccountRow {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub family: String,
    pub follows_app: bool,
    /// `profile.status` once loaded.
    pub status: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    Interrupt(String),
    Quit,
    /// Merge back, step 1: commit the worktree and merge the target into the agent's branch.
    MergePrepare { run: String, text: String },
    /// Merge back, step 2: merge the agent's branch into the target in the source checkout.
    MergeComplete { run: String, text: String },
    /// Remove a finished agent's worktree (its branch is kept).
    Cleanup { run: String, text: String, discard: bool },
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
    Comparisons { run: String },
    Diff { run: String },
    AccountList,
    AccountStatus(String),
    Login(String),
    MergePlan { run: String },
    MergeResolved { run: String },
    MergePrepare { run: String },
    MergeLanding { run: String, branch: String, target: String, repo: String },
    MergeFiles { run: String, text: String },
    MergeComplete,
    CleanupPlan { run: String },
    Cleanup,
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

/// The Changes view (`v`): what an agent changed, like the review in VS Code.
#[derive(Debug, Clone, Default)]
pub struct ChangesView {
    pub run: String,
    /// Available comparisons: (label, base commit).
    pub options: Vec<(String, String)>,
    pub option: usize,
    /// Changed files: (status letter, path, added lines, removed lines).
    pub files: Vec<(String, String, u64, u64)>,
    pub file: usize,
    /// Diff of the selected file (unified, without color codes).
    pub diff: Vec<String>,
    pub scroll: usize,
    /// Tree object of the worktree as captured by the daemon (includes untracked files).
    pub tree: Option<String>,
    pub root: String,
    pub error: Option<String>,
    pub loading: bool,
}

/// Lines of diff shown for one file at most.
const DIFF_LINES: usize = 4000;

fn git_out(dir: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
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
    pub changes: ChangesView,
    /// Search text (`/`): agents whose title, repository, harness, model or prompt contain it.
    pub search: String,
    pub accounts: Vec<AccountRow>,
    pub account_sel: usize,
    /// Set when a program must run with the terminal (the event loop suspends the TUI for it).
    pub exec: Option<Exec>,
    /// Zoom shows tool inputs and results under each tool call.
    pub expand_tools: bool,
    /// Agents waiting for you at the last state (to notice new ones).
    waiting: HashSet<String>,
    /// Ring the terminal bell (an agent started waiting for you); the event loop clears it.
    pub bell: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub draws: u64,
    pub events: u64,
    /// Milliseconds from each event's daemon timestamp to the app handling it (bounded).
    pub lag_ms: Vec<i64>,
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
            changes: ChangesView::default(),
            search: String::new(),
            accounts: Vec::new(),
            account_sel: 0,
            exec: None,
            expand_tools: false,
            waiting: HashSet::new(),
            bell: false,
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
        let q = self.search.trim().to_lowercase();
        self.state.agents().into_iter().filter(|r| self.filter.keeps(r) && (q.is_empty() || self.matches(r, &q))).collect()
    }

    fn matches(&self, r: &Run, q: &str) -> bool {
        let task = self.state.task(&r.task_id);
        let repo = task.map(|t| t.repo_root.rsplit('/').next().unwrap_or_default()).unwrap_or_default();
        let account = r.profile_id.as_deref().and_then(|p| self.state.profile(p)).map(|p| p.name.as_str()).unwrap_or_default();
        [r.title.as_str(), repo, r.harness.as_str(), r.model.as_deref().unwrap_or_default(), account, task.map(|t| t.prompt.as_str()).unwrap_or_default(), r.status.as_str()]
            .iter()
            .any(|f| f.to_lowercase().contains(q))
    }

    fn search_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {
                self.search.clear();
                self.mode = Mode::Grid;
            }
            KeyCode::Enter => self.mode = Mode::Grid,
            KeyCode::Backspace => {
                self.search.pop();
            }
            KeyCode::Char(c) if !c.is_control() => self.search.push(c),
            _ => return,
        }
        self.page = 0;
        self.focus = None;
        self.settle_focus();
        self.ensure_history();
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
        let on_page = |p: usize| n.saturating_sub(p * PAGE).min(PAGE);
        let (_, cols) = shape(on_page(page));
        let (row, col) = ((slot / cols) as i32, (slot % cols) as i32);
        let target = if dx != 0 {
            let c = col + dx;
            if c < 0 {
                // Past the left edge: the previous page's same row, rightmost column.
                if page == 0 {
                    None
                } else {
                    let (_, pc) = shape(on_page(page - 1));
                    Some((page - 1) * PAGE + (row as usize) * pc + pc - 1)
                }
            } else if c >= cols as i32 {
                // Past the right edge: the next page's same row (or its last agent).
                if on_page(page + 1) == 0 {
                    None
                } else {
                    let (nr, nc) = shape(on_page(page + 1));
                    Some(((page + 1) * PAGE + (row as usize).min(nr - 1) * nc).min(n - 1))
                }
            } else {
                Some(page * PAGE + (row * cols as i32 + c) as usize)
            }
        } else {
            let r = row + dy;
            let t = page * PAGE + (r.max(0) as usize) * cols + col as usize;
            if r >= 0 && t < page * PAGE + on_page(page) { Some(t) } else { None }
        };
        if let Some(t) = target {
            if t < n {
                self.focus_index(t);
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

    /// Where a feed's paths are shortened from: its workspace and repository.
    fn locate_feed(state: &State, root: &str, feed: &mut Feed) {
        if feed.root.is_none() {
            feed.root = state.run(root).and_then(|r| state.workspace(&r.workspace_id)).map(|w| w.path.clone());
        }
        if feed.repo.is_none() {
            feed.repo = state.run(root).and_then(|r| state.task(&r.task_id)).map(|t| (t.repo_root.clone(), t.repo_root.rsplit('/').next().unwrap_or_default().to_string()));
        }
    }

    fn on_event(&mut self, ev: Value) {
        self.stats.events += 1;
        if let Some(ts) = ev["ts"].as_i64() {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
            if self.stats.lag_ms.len() < 100_000 {
                self.stats.lag_ms.push(now - ts);
            }
        }
        let run_id = ev["run_id"].as_str().unwrap_or_default().to_string();
        let kind = ev["kind"].as_str().unwrap_or_default().to_string();
        if !run_id.is_empty() {
            let root = self.state.root_of(&run_id);
            let child = if root != run_id { Some(self.state.run(&run_id).map(|r| r.title.clone()).unwrap_or_else(|| "sub-agent".into())) } else { None };
            let feed = self.feeds.entry(root.clone()).or_default();
            Self::locate_feed(&self.state, &root, feed);
            feed.add(&ev, child.as_deref());
            self.last_event.insert(root, Instant::now());
        }
        // Statuses, turns and new runs come from `state`, reloaded like VS Code does.
        if (matches!(kind.as_str(), "status" | "turn_started" | "turn_done" | "permission" | "permission_answered" | "child" | "child_reparented" | "task_created" | "workspace_removed" | "reattached")
            || (!run_id.is_empty() && self.state.run(&run_id).is_none()))
            && self.state_due.is_none()
        {
            self.state_due = Some(Instant::now() + Duration::from_millis(80));
        }
    }

    fn on_reply(&mut self, why: Pending, result: Result<Value, String>) {
        match (why, result) {
            (Pending::State, Ok(v)) => {
                self.state_inflight = false;
                match serde_json::from_value::<State>(v) {
                    Ok(state) => {
                        let cursor = state.cursor;
                        let now_waiting: HashSet<String> = state.runs.iter().filter(|r| r.parent_run_id.is_none() && r.needs_you()).map(|r| r.id.clone()).collect();
                        let first_load = self.state.runs.is_empty();
                        let new: Vec<String> = now_waiting.difference(&self.waiting).cloned().collect();
                        self.waiting = now_waiting;
                        self.state = state;
                        if !first_load && !new.is_empty() {
                            // Someone needs you: a bell, and a pointer to it unless it is already focused.
                            self.bell = true;
                            if new.iter().all(|id| Some(id.as_str()) != self.focus.as_deref()) {
                                let name = self.state.run(&new[0]).map(|r| r.title.clone()).unwrap_or_default();
                                let more = if new.len() > 1 { format!(" and {} more", new.len() - 1) } else { String::new() };
                                self.say(format!("◆ {}{more} needs you — press w", short(&name, 40)), false);
                            }
                        }
                        for (root, feed) in self.feeds.iter_mut() {
                            Self::locate_feed(&self.state, root, feed);
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
                let feed = self.feeds.entry(root.clone()).or_default();
                Self::locate_feed(&self.state, &root, feed);
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
            (Pending::Comparisons { run }, Ok(v)) => {
                if self.changes.run != run {
                    return;
                }
                let opts: Vec<(String, String)> = v["options"].as_array().cloned().unwrap_or_default().iter()
                    .filter(|o| o["available"].as_bool().unwrap_or(false) && o["base"].is_string())
                    .map(|o| (o["label"].as_str().unwrap_or("comparison").to_string(), o["base"].as_str().unwrap_or_default().to_string()))
                    .collect();
                if opts.is_empty() {
                    self.changes.loading = false;
                    self.changes.error = Some("No comparison is available for this agent yet.".into());
                    return;
                }
                self.changes.options = opts;
                self.changes.option = 0;
                self.load_diff();
            }
            (Pending::Diff { run }, Ok(v)) => {
                if self.changes.run != run {
                    return;
                }
                self.changes.loading = false;
                self.changes.root = v["root"].as_str().unwrap_or_default().to_string();
                self.changes.tree = v["current_tree"].as_str().map(str::to_string);
                let base = v["base"].as_str().unwrap_or_default().to_string();
                let mut counts: HashMap<String, (u64, u64)> = HashMap::new();
                if let Some(tree) = &self.changes.tree {
                    if let Ok(num) = git_out(&self.changes.root, &["diff", "--numstat", "-M", &base, tree]) {
                        for l in num.lines() {
                            let mut it = l.split('\t');
                            let (a, d, path) = (it.next().unwrap_or("0"), it.next().unwrap_or("0"), it.next_back().unwrap_or_default());
                            counts.insert(path.to_string(), (a.parse().unwrap_or(0), d.parse().unwrap_or(0)));
                        }
                    }
                }
                let keep = self.changes.files.get(self.changes.file).map(|f| f.1.clone());
                self.changes.files = v["changes"].as_array().cloned().unwrap_or_default().iter().map(|c| {
                    let path = c["path"].as_str().unwrap_or_default().to_string();
                    let (a, d) = counts.get(&path).copied().unwrap_or((0, 0));
                    (c["status"].as_str().unwrap_or("M").to_string(), path, a, d)
                }).collect();
                self.changes.file = keep.and_then(|k| self.changes.files.iter().position(|f| f.1 == k)).unwrap_or(0);
                self.changes.error = None;
                self.load_file_diff();
            }
            (Pending::AccountList, Ok(v)) => {
                let keep = self.accounts.get(self.account_sel).map(|a| a.id.clone());
                self.accounts = v["accounts"].as_array().cloned().unwrap_or_default().iter().map(|a| AccountRow {
                    id: a["id"].as_str().unwrap_or_default().to_string(),
                    name: a["name"].as_str().unwrap_or_default().to_string(),
                    provider: a["provider"].as_str().unwrap_or_default().to_string(),
                    family: a["harness_family"].as_str().unwrap_or_default().to_string(),
                    follows_app: a["kind"] == "follows-app",
                    status: self.accounts.iter().find(|o| o.id == a["id"].as_str().unwrap_or_default()).and_then(|o| o.status.clone()),
                }).collect();
                // Grouped by provider, as in VS Code.
                let rank = |p: &str| match p { "openai" => 0, "anthropic" => 1, _ => 2 };
                self.accounts.sort_by(|a, b| rank(&a.provider).cmp(&rank(&b.provider)).then(b.follows_app.cmp(&a.follows_app)).then(a.name.cmp(&b.name)));
                self.account_sel = keep.and_then(|k| self.accounts.iter().position(|a| a.id == k)).unwrap_or(0);
                let ids: Vec<String> = self.accounts.iter().map(|a| a.id.clone()).collect();
                for id in ids {
                    self.request("profile.status", json!({ "id": id }), Pending::AccountStatus(id.clone()));
                }
            }
            (Pending::AccountStatus(id), Ok(v)) => {
                if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) {
                    a.status = Some(v);
                }
            }
            (Pending::Login(name), Ok(v)) => {
                let env: Vec<(String, String)> = v["env"].as_object().map(|m| m.iter().filter_map(|(k, x)| x.as_str().map(|x| (k.clone(), x.to_string()))).collect()).unwrap_or_default();
                self.exec = Some(Exec {
                    title: format!("Signing in {name}"),
                    program: v["program"].as_str().unwrap_or_default().to_string(),
                    args: v["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                    env,
                });
            }
            (Pending::MergePlan { run }, Ok(plan)) => self.on_merge_plan(run, plan),
            (Pending::CleanupPlan { run }, Ok(plan)) => {
                if plan["removable"] != true {
                    self.say(format!("Not removing: {}", plan["reason"].as_str().unwrap_or("not removable")), true);
                    return;
                }
                let d = &plan["dirty"];
                let mut files: Vec<String> = Vec::new();
                for key in ["staged", "unstaged", "conflicted"] {
                    for f in d[key].as_array().into_iter().flatten() {
                        files.push(f["path"].as_str().or(f.as_str()).unwrap_or_default().to_string());
                    }
                }
                for f in d["untracked"].as_array().into_iter().flatten() {
                    files.push(f.as_str().unwrap_or_default().to_string());
                }
                let branch = plan["workspace"]["branch"].as_str().unwrap_or("its branch").to_string();
                let text = if files.is_empty() {
                    format!("Remove this worktree? {branch} is kept; no uncommitted work.")
                } else {
                    format!("Remove this worktree? {branch} is kept, but {} uncommitted file{} will be LOST: {}.", files.len(), if files.len() == 1 { "" } else { "s" }, files.iter().take(6).cloned().collect::<Vec<_>>().join(", "))
                };
                self.mode = Mode::Confirm(Confirm::Cleanup { run, text, discard: !files.is_empty() });
            }
            (Pending::Cleanup, Ok(_)) => {
                self.say("Worktree removed; the branch is kept", false);
                self.request_state();
            }
            (Pending::MergeResolved { run }, Ok(v)) => {
                if v["state"] == "ready" {
                    self.merge_plan(&run);
                } else {
                    let left: Vec<String> = v["remaining"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    self.say(format!("Conflict markers remain in {}. Resolve them (or ask the agent), then press M again.", left.join(", ")), true);
                }
            }
            (Pending::MergePrepare { run }, Ok(v)) => {
                if v["state"] == "conflicts" {
                    let files: Vec<String> = v["files"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    let how = if v["handoff"]["sent"] == true { "sent to the agent as a follow-up; press M again when it finishes".to_string() } else { format!("resolve them in the worktree ({}), then press M again", v["handoff"]["why"].as_str().unwrap_or("no follow-up possible")) };
                    self.say(format!("Merge back: conflicts in {} — {how}", files.join(", ")), true);
                    self.request_state();
                } else {
                    self.merge_plan(&run);
                }
            }
            (Pending::MergeLanding { run, branch, target, repo }, Ok(v)) => {
                let landing = v["options"].as_array().and_then(|a| a.iter().find(|o| o["mode"] == "branch_merge_base" && o["branch"] == target.as_str() && o["available"] == true).cloned());
                let ws = self.state.run(&run).map(|r| r.workspace_id.clone()).unwrap_or_default();
                let repo_name = repo.rsplit('/').next().unwrap_or_default().to_string();
                let text = format!("Merge {branch} into {target} in {repo_name}?");
                match landing.and_then(|o| o["base"].as_str().map(str::to_string)) {
                    Some(base) => self.request("workspace.diff", json!({ "workspace_id": ws, "base": base, "status": false }), Pending::MergeFiles { run, text }),
                    None => self.mode = Mode::Confirm(Confirm::MergeComplete { run, text }),
                }
            }
            (Pending::MergeFiles { run, text }, Ok(v)) => {
                let n = v["changes"].as_array().map(|a| a.len()).unwrap_or(0);
                self.mode = Mode::Confirm(Confirm::MergeComplete { run, text: format!("{text} {n} file{} land{}. The worktree and branch are kept.", if n == 1 { "" } else { "s" }, if n == 1 { "s" } else { "" }) });
            }
            (Pending::MergeComplete, Ok(v)) => {
                self.say(format!("Merged {} into {} ({}). The worktree and branch are kept.", v["branch"].as_str().unwrap_or("the branch"), v["target"].as_str().unwrap_or("the target"), v["commit"].as_str().unwrap_or_default().chars().take(10).collect::<String>()), false);
                self.request_state();
            }
            (Pending::Comparisons { .. } | Pending::Diff { .. }, Err(e)) => {
                self.changes.loading = false;
                self.changes.error = Some(e);
            }
            (Pending::Create, Err(e)) => {
                self.form.busy = false;
                self.form.error = Some(e);
            }
            (_, Err(e)) => self.say(e, true),
        }
    }

    // ---------------------------------------------------------------- actions

    /// The terminal window title: counts that matter when the TUI is in another tab.
    pub fn window_title(&self) -> String {
        let all = self.state.agents();
        let needs = all.iter().filter(|r| r.needs_you()).count();
        let active = all.iter().filter(|r| r.active()).count();
        match (needs, active) {
            (0, 0) => "Overseer".into(),
            (0, a) => format!("Overseer · {a} active"),
            (n, a) => format!("Overseer · {n} need{} you · {a} active", if n == 1 { "s" } else { "" }),
        }
    }

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

    fn open_changes(&mut self) {
        let Some(run) = self.focused().cloned() else { return };
        self.changes = ChangesView { run: run.id.clone(), loading: true, ..Default::default() };
        self.mode = Mode::Changes;
        self.request("comparison.options", json!({ "run_id": run.id }), Pending::Comparisons { run: run.id.clone() });
    }

    fn load_diff(&mut self) {
        let run = self.changes.run.clone();
        let Some(ws) = self.state.run(&run).map(|r| r.workspace_id.clone()) else { return };
        let Some((_, base)) = self.changes.options.get(self.changes.option).cloned() else { return };
        self.changes.loading = true;
        self.request("workspace.diff", json!({ "workspace_id": ws, "base": base, "status": false }), Pending::Diff { run });
    }

    /// The selected file's unified diff (read-only `git diff` between the base and the captured tree).
    fn load_file_diff(&mut self) {
        let c = &mut self.changes;
        c.scroll = 0;
        c.diff.clear();
        let (Some((_, base)), Some(tree), Some(file)) = (c.options.get(c.option), c.tree.as_ref(), c.files.get(c.file)) else { return };
        let mut args = vec!["diff", "--no-color", "-M", base.as_str(), tree.as_str(), "--"];
        let old = file.1.clone();
        args.push(&old);
        match git_out(&c.root, &args) {
            Ok(text) => {
                c.diff = text.lines().skip_while(|l| !l.starts_with("@@") && !l.starts_with("Binary")).take(DIFF_LINES).map(str::to_string).collect();
                if c.diff.is_empty() {
                    c.diff.push("(no textual change)".into());
                }
            }
            Err(e) => c.error = Some(e),
        }
    }

    fn merge_plan(&mut self, run: &str) {
        let Some(ws) = self.state.run(run).map(|r| r.workspace_id.clone()) else { return };
        self.request("workspace.merge_plan", json!({ "workspace_id": ws }), Pending::MergePlan { run: run.to_string() });
    }

    /// Merge back, like VS Code's: never automatic; each step is confirmed.
    fn on_merge_plan(&mut self, run: String, plan: Value) {
        if plan["ok"] != true {
            self.say(format!("Merge back is unavailable: {}", plan["reason"].as_str().unwrap_or("unknown reason")), true);
            return;
        }
        let branch = plan["branch"].as_str().unwrap_or_default().to_string();
        let target = plan["target"].as_str().unwrap_or_default().to_string();
        let repo = plan["repo"].as_str().unwrap_or_default().to_string();
        match plan["state"].as_str().unwrap_or_default() {
            "idle" => {
                let n = plan["worktree_uncommitted"].as_array().map(|a| a.len()).unwrap_or(0);
                let commit = if n > 0 { format!("commit {n} worktree file{} and ", if n == 1 { "" } else { "s" }) } else { String::new() };
                self.mode = Mode::Confirm(Confirm::MergePrepare { run, text: format!("Merge back {branch} → {target}: {commit}merge {target} into {branch} in the worktree (conflicts go back to the agent)?") });
            }
            "resolving" | "resolved" => {
                let ws = self.state.run(&run).map(|r| r.workspace_id.clone()).unwrap_or_default();
                self.request("workspace.merge_resolved", json!({ "workspace_id": ws }), Pending::MergeResolved { run });
            }
            "ready" => {
                let blockers: Vec<String> = plan["blockers"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                if !blockers.is_empty() {
                    self.say(format!("Merge back is ready but blocked: {}", blockers.join(" ")), true);
                    return;
                }
                self.request("comparison.options", json!({ "run_id": run, "branch": target }), Pending::MergeLanding { run, branch, target, repo });
            }
            other => self.say(format!("Merge back: unexpected state {other}"), true),
        }
    }

    fn open_accounts(&mut self) {
        self.mode = Mode::Accounts;
        self.request("account.list", json!({}), Pending::AccountList);
    }

    /// Called by the event loop after a suspended program (a sign-in) finished.
    pub fn after_exec(&mut self, result: Result<i32, String>) {
        match result {
            Ok(0) => self.say("Sign-in finished", false),
            Ok(code) => self.say(format!("Sign-in exited with code {code}"), true),
            Err(e) => self.say(format!("Could not run the sign-in: {e}"), true),
        }
        if self.mode == Mode::Accounts {
            self.request("account.list", json!({}), Pending::AccountList);
        }
        self.dirty = true;
    }

    fn accounts_key(&mut self, k: KeyEvent) {
        let n = self.accounts.len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('A') | KeyCode::Char('q') => self.mode = Mode::Grid,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.account_sel = (self.account_sel + 1) % n,
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.account_sel = (self.account_sel + n - 1) % n,
            KeyCode::Char('r') => self.request("account.list", json!({}), Pending::AccountList),
            KeyCode::Char(c @ ('s' | 'S')) => {
                let Some(a) = self.accounts.get(self.account_sel).cloned() else { return };
                if a.provider == "local" {
                    self.say("Local model accounts have no sign-in", false);
                    return;
                }
                // S: ChatGPT's device-code sign-in (for another browser or device).
                let device = c == 'S' && a.family == "codex";
                self.request("profile.login_command", json!({ "id": a.id, "device": device }), Pending::Login(a.name.clone()));
            }
            _ => {}
        }
    }

    fn changes_key(&mut self, k: KeyEvent) {
        let n = self.changes.files.len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('v') | KeyCode::Char('q') => self.mode = Mode::Grid,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => {
                self.changes.file = (self.changes.file + 1) % n;
                self.load_file_diff();
            }
            KeyCode::Up | KeyCode::Char('k') if n > 0 => {
                self.changes.file = (self.changes.file + n - 1) % n;
                self.load_file_diff();
            }
            KeyCode::Char('J') | KeyCode::PageDown => self.changes.scroll = (self.changes.scroll + (self.size.1 as usize).saturating_sub(6)).min(self.changes.diff.len().saturating_sub(1)),
            KeyCode::Char('K') | KeyCode::PageUp => self.changes.scroll = self.changes.scroll.saturating_sub((self.size.1 as usize).saturating_sub(6)),
            KeyCode::Char('c') if !self.changes.options.is_empty() => {
                self.changes.option = (self.changes.option + 1) % self.changes.options.len();
                self.changes.files.clear();
                self.load_diff();
            }
            KeyCode::Char('r') => self.load_diff(),
            _ => {}
        }
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
                        Confirm::MergePrepare { run, .. } => {
                            if let Some(ws) = self.state.run(&run).map(|r| r.workspace_id.clone()) {
                                self.say("Preparing merge back…", false);
                                self.request("workspace.merge_prepare", json!({ "workspace_id": ws, "handoff": true }), Pending::MergePrepare { run });
                            }
                        }
                        Confirm::Cleanup { run, discard, .. } => {
                            if let Some(ws) = self.state.run(&run).map(|r| r.workspace_id.clone()) {
                                self.request("workspace.cleanup", json!({ "workspace_id": ws, "discard_dirty": discard }), Pending::Cleanup);
                            }
                        }
                        Confirm::MergeComplete { run, .. } => {
                            if let Some(ws) = self.state.run(&run).map(|r| r.workspace_id.clone()) {
                                self.request("workspace.merge_complete", json!({ "workspace_id": ws }), Pending::MergeComplete);
                            }
                        }
                    }
                }
                _ => self.mode = Mode::Grid,
            },
            Mode::Compose => self.compose_key(k),
            Mode::NewAgent => self.form_key(k),
            Mode::Changes => self.changes_key(k),
            Mode::Search => self.search_key(k),
            Mode::Accounts => self.accounts_key(k),
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
            KeyCode::Char('v') => self.open_changes(),
            KeyCode::Char('A') => self.open_accounts(),
            KeyCode::Char('C') => {
                if let Some(run) = self.focused().cloned() {
                    if run.active() {
                        self.say("Cleaning up waits until the agent is done", false);
                    } else {
                        let ws = run.workspace_id.clone();
                        self.request("workspace.cleanup_plan", json!({ "workspace_id": ws }), Pending::CleanupPlan { run: run.id.clone() });
                    }
                }
            }
            KeyCode::Char('M') => {
                if let Some(run) = self.focused().cloned() {
                    if run.active() {
                        self.say("Merge back waits until the agent is done (x interrupts it)", false);
                    } else {
                        self.merge_plan(&run.id);
                    }
                }
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.page = 0;
            }
            KeyCode::Esc if !self.search.is_empty() => {
                self.search.clear();
                self.settle_focus();
                self.ensure_history();
            }
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
            KeyCode::Char('e') if zoom => self.expand_tools = !self.expand_tools,
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
            KeyCode::Char(c) if !c.is_control() => self.drafts.entry(id).or_default().push(c),
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
            KeyCode::Char(c) if !c.is_control() => {
                if self.form.field == 0 && (c == '/' || c == '~') {
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

/// Grid shape (rows, columns) for `n` agents on a page: 1, 1×2, 1×3, 2×2, 2×3, 3×3.
pub fn shape(n: usize) -> (usize, usize) {
    match n {
        0 | 1 => (1, 1),
        2 => (1, 2),
        3 => (1, 3),
        4 => (2, 2),
        5 | 6 => (2, 3),
        _ => (3, 3),
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
