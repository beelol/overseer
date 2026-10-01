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

mod phone;
pub use phone::{ago, code_groups, fingerprint, platform_name, Device, PairRequest, Pairing, PairingState, Phone};

/// Agents on one screen at most (T-37); from the 17th the grid pages.
pub const PAGE: usize = 16;
/// The smallest tile the grid narrows to before it takes fewer columns (T-37).
pub const MIN_TILE_W: u16 = 22;
pub const MIN_TILE_H: u16 = 5;
/// Pages of history fetched per run (5,000 events each), newest kept by the feed cap.
const HISTORY_PAGES: usize = 10;
const MAX_AUDIO_IMPORT_PATH: usize = 4096;
/// How often the daemon is asked for its audio settings while connected, so a change made in
/// another client counts within 2 s (T-23, T-24).
const AUDIO_REFRESH: Duration = Duration::from_millis(1000);
/// The same after an error (a daemon without audio methods): rarely.
const AUDIO_RETRY: Duration = Duration::from_secs(30);

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
    /// Paired phones (`D`): revoke, scope, pair a phone.
    Devices,
    /// The pairing code as text and as a QR code, and the time it still works.
    Pairing,
    /// Daemon-owned Audio Mode settings and cue previews.
    Audio,
    /// Private Commander folder path entry.
    AudioImport,
    /// The conversation with Overseer (`o`): its messages, the proposals that wait, a composer.
    Overseer,
}

/// A program to run in the terminal with the TUI suspended (a provider's own sign-in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exec {
    pub title: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The owner's editor from the review (T-39), not a sign-in.
    pub edit: bool,
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
    /// Reject changes in the review: the comparison's lines go back into the worktree (T-29).
    Reject { path: String, keys: Vec<String>, text: String },
    /// Remove a finished agent's worktree (its branch is kept).
    Cleanup { run: String, text: String, discard: bool },
    /// Interrupt every agent and stop the daemon.
    StopAll { text: String },
    /// Commit, push the agent's branch and open a GitHub pull request with `gh`.
    OpenPr { run: String, text: String },
    /// Turn phone access off while phones are connected.
    PhoneOff { text: String },
    /// Pairing was asked for with phone access off: turn it on first.
    PhoneOnAndPair,
    /// Revoke a paired phone.
    Revoke { id: String, name: String, text: String },
    /// A phone asks to pair: the owner's decision.
    Pair { request: String, text: String },
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
    Hunks { run: String, path: String },
    FileText { run: String, path: String },
    Tree { run: String },
    Accept { run: String, path: String, rest: Vec<Hunk> },
    Reject { run: String, path: String, rest: Vec<String> },
    AccountList,
    AccountStatus(String),
    Login(String),
    MergePlan { run: String },
    MergeResolved { run: String },
    MergePrepare { run: String },
    MergeLanding { run: String, branch: String, target: String, repo: String },
    MergeFiles { run: String, text: String },
    MergeComplete,
    /// `review.seen`: a reviewed mark shared with VS Code and the menu bar (T-26).
    Seen,
    CleanupPlan { run: String },
    Cleanup,
    StopAll,
    PrPlan { run: String },
    PrPrepare { run: String, plan: Value },
    PrPublish { run: String },
    PrOpened,
    PhoneStatus,
    PhoneSwitch { on: bool, then_pair: bool },
    PairStart,
    PairCancel,
    PairConfirm { accept: bool, name: String },
    DeviceRevoke(String),
    DeviceScope { name: String, scope: String },
    PhoneNotifications(bool),
    AudioGet,
    AudioSet,
    AudioPreview,
    OverseerSession,
    OverseerSend,
    OverseerAnswer,
    AudioVoices,
    AudioImport,
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

/// A comparison the review can show (`comparison.options`): available or not, and why.
#[derive(Debug, Clone, Default)]
pub struct Comparison {
    pub mode: String,
    pub label: String,
    pub base: Option<String>,
    pub available: bool,
    pub detail: String,
}

/// One change of a file, as the daemon names it (`workspace.hunks`): its key, the comparison's
/// lines it replaces and the working copy's lines, and whether it is accepted.
#[derive(Debug, Clone, Default)]
pub struct Hunk {
    pub key: String,
    pub base_start: usize,
    pub base_lines: Vec<String>,
    pub modified_start: usize,
    pub modified_lines: Vec<String>,
    pub reviewed: bool,
}

/// The comparisons that `1`, `2` and `3` switch to (T-27).
pub const REVIEW_KEYS: [(&str, &str); 3] = [("task_start", "Since task start"), ("latest_run", "Latest run"), ("entire_worktree", "Entire worktree")];

/// The review (`v`): what an agent changed, like the review in VS Code, all through the daemon:
/// the comparisons, the files, each change with its Accept and Reject.
#[derive(Debug, Clone, Default)]
pub struct ChangesView {
    pub run: String,
    pub workspace: String,
    /// Every comparison the daemon offers, available or not.
    pub options: Vec<Comparison>,
    pub option: usize,
    /// Changed files: (status letter, path, added lines, removed lines).
    pub files: Vec<(String, String, u64, u64)>,
    /// `t`: All files (the whole worktree, from the daemon) instead of the changed ones (T-28).
    pub all_files: bool,
    /// Every file of the worktree, once listed.
    pub all: Vec<String>,
    pub all_loading: usize,
    pub file: usize,
    /// The selected file's changes and the lines drawn for them.
    pub hunks: Vec<Hunk>,
    pub change: usize,
    pub diff: Vec<String>,
    /// Where each change starts in `diff`.
    pub hunk_at: Vec<usize>,
    /// The selected file is unchanged: its contents, read-only.
    pub unchanged: bool,
    pub scroll: usize,
    /// Tree object of the worktree as captured by the daemon (includes untracked files).
    pub tree: Option<String>,
    pub root: String,
    pub error: Option<String>,
    pub loading: bool,
    /// The file whose changes are shown.
    pub path: String,
    /// The selected file's working copy, by line.
    pub now: Vec<String>,
    /// Lines the owner wrote in their own editor (`e`, T-39), per file: shown as theirs.
    pub mine: HashMap<String, HashSet<String>>,
    /// The file as it was when the editor opened, to tell the owner's lines from the agent's.
    pub editing: Option<(String, String)>,
}

impl ChangesView {
    /// The file list shown: the changed files, or every file with the changed ones' counts.
    pub fn shown(&self) -> Vec<(String, String, u64, u64)> {
        if !self.all_files {
            return self.files.clone();
        }
        let mut out: Vec<(String, String, u64, u64)> = self.all.iter().map(|p| self.files.iter().find(|f| &f.1 == p).cloned().unwrap_or_else(|| (String::new(), p.clone(), 0, 0))).collect();
        for f in &self.files {
            if !self.all.contains(&f.1) {
                out.push(f.clone());
            }
        }
        out.sort_by(|a, b| a.1.cmp(&b.1));
        out
    }

    pub fn selected(&self) -> Option<(String, String, u64, u64)> {
        self.shown().get(self.file).cloned()
    }

    pub fn comparison(&self) -> Option<&Comparison> {
        self.options.get(self.option)
    }

    pub fn accepted(&self) -> usize {
        self.hunks.iter().filter(|h| h.reviewed).count()
    }
}

/// What the daemon last said about Audio Mode. The TUI keeps no audio setting of its own: the
/// daemon owns the settings and the playback.
#[derive(Debug, Clone)]
pub struct AudioSettings {
    /// The daemon's latest answer is in the fields below. False before the first answer, after an
    /// answer with an error and while disconnected.
    pub known: bool,
    pub enabled: bool,
    pub available: bool,
    pub track: String,
    pub voice: String,
    pub commander_imported: bool,
    pub voices: Vec<String>,
    pub preview: usize,
    pub import_path: String,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { known: false, enabled: false, available: false, track: "reactor".into(),
            voice: String::new(), commander_imported: false, voices: Vec::new(), preview: 0,
            import_path: String::new() }
    }
}

impl AudioSettings {
    pub const CORE_KEYS: [&'static str; 3] = ["agent_started", "agent_complete", "agent_needs_attention"];

    /// Takes the daemon's answer; true when it differs from what was known.
    fn update(&mut self, value: &Value) -> bool {
        let before = (self.known, self.enabled, self.available, self.track.clone(), self.voice.clone(), self.commander_imported);
        self.known = true;
        self.enabled = value["enabled"].as_bool().unwrap_or(false);
        self.available = value["available"].as_bool().unwrap_or(false);
        self.track = value["track"].as_str().unwrap_or("reactor").to_string();
        self.voice = value["voice"].as_str().unwrap_or_default().to_string();
        self.commander_imported = value["commander_imported"].as_bool().unwrap_or(false);
        before != (self.known, self.enabled, self.available, self.track.clone(), self.voice.clone(), self.commander_imported)
    }

    /// The daemon plays the cue for an agent that needs you, so the terminal bell stays quiet
    /// (T-24). Only when its latest answer says so: in every other case the bell rings.
    pub fn plays(&self) -> bool {
        self.known && self.enabled && self.available
    }

    pub fn preview_key(&self) -> &'static str {
        Self::CORE_KEYS[self.preview % Self::CORE_KEYS.len()]
    }
}

/// Lines of diff shown for one file at most.
const DIFF_LINES: usize = 4000;
/// Lines of the working copy shown around each change.
const CONTEXT: usize = 2;
/// Files listed in All files at most (T-28).
const ALL_FILES_MAX: usize = 5000;

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
    /// Agent list rows from the last draw (T-25): a click picks the agent.
    pub list_hit: Vec<(String, u16, u16, u16, u16)>,
    /// The focused agent's conversation shows in a column beside the grid (T-25, way 2).
    pub picked: bool,
    /// Lines up from the bottom in that conversation (0 = following).
    pub conv_scroll: usize,
    /// `L` hides the agent list.
    pub list_hidden: bool,
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
    /// The daemon's conversation with Overseer, as last loaded, and the words being typed to it.
    pub overseer: Value,
    pub overseer_draft: String,
    pub overseer_scroll: usize,
    pub accounts: Vec<AccountRow>,
    /// Each account's login read once after the first state (AC-235), so every tile names its
    /// account even when VS Code has not asked yet.
    accounts_read: bool,
    pub account_sel: usize,
    pub audio: AudioSettings,
    /// When to ask the daemon for its audio settings again.
    audio_due: Option<Instant>,
    audio_inflight: bool,
    /// `S` was pressed before the daemon's first answer: open the panel when it arrives.
    audio_wanted: bool,
    /// Set when a program must run with the terminal (the event loop suspends the TUI for it).
    pub exec: Option<Exec>,
    /// Zoom shows tool inputs and results under each tool call.
    pub expand_tools: bool,
    /// Agents waiting for you at the last state (to notice new ones).
    waiting: HashSet<String>,
    /// Ring the terminal bell (an agent started waiting for you); the event loop clears it.
    pub bell: bool,
    /// Agents and daemon were stopped on purpose; `r` starts the daemon again.
    pub stopped: bool,
    /// Where background jobs (a push, `gh`) report back, as replies with their own ids.
    jobs: Option<std::sync::mpsc::Sender<Msg>>,
    next_job: u64,
    /// The last Open PR plan (kept between the confirmation and the prepare step).
    pr_plan: Option<Value>,
    /// Phone access as the daemon reports it (`O`, `D`).
    pub phone: Phone,
    /// The pairing this terminal started, while its panel is open.
    pub pairing: Option<Pairing>,
    /// Where a phone question returns to (the Devices or pairing panel).
    pub confirm_back: Option<Mode>,
    phone_due: Option<Instant>,
    /// Pairing requests already put to the owner here.
    pair_asked: HashSet<String>,
    /// `gateway.pair_start` calls whose `pairing_opened` event is still to come.
    pair_starting: u32,
    pairing_since: Option<Instant>,
    /// The seconds left last drawn (the clock redraws once a second).
    pairing_shown: u64,
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
            list_hit: Vec::new(),
            picked: false,
            conv_scroll: 0,
            list_hidden: false,
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
            overseer: Value::Null,
            overseer_draft: String::new(),
            overseer_scroll: 0,
            accounts: Vec::new(),
            accounts_read: false,
            account_sel: 0,
            audio: AudioSettings::default(),
            audio_due: None,
            audio_inflight: false,
            audio_wanted: false,
            exec: None,
            expand_tools: false,
            waiting: HashSet::new(),
            bell: false,
            stopped: false,
            jobs: None,
            next_job: 1 << 60,
            pr_plan: None,
            phone: Phone::default(),
            pairing: None,
            confirm_back: None,
            phone_due: None,
            pair_asked: HashSet::new(),
            pair_starting: 0,
            pairing_since: None,
            pairing_shown: 0,
        }
    }

    /// Lets background jobs report to the event loop (their replies arrive like daemon replies).
    pub fn set_jobs(&mut self, tx: std::sync::mpsc::Sender<Msg>) {
        self.jobs = Some(tx);
    }

    /// Runs slow work (network) off the event loop; its result arrives as a reply.
    fn job(&mut self, why: Pending, work: impl FnOnce() -> Result<Value, String> + Send + 'static) {
        self.next_job += 1;
        let id = self.next_job;
        self.pending.insert(id, why);
        match self.jobs.clone() {
            Some(tx) => {
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Reply { id, result: work() });
                });
            }
            None => {
                let result = work();
                let why = self.pending.remove(&id).unwrap();
                self.on_reply(why, result);
            }
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
        let account = r.profile_id.as_deref().and_then(|p| self.state.profile(p)).map(|p| p.label()).unwrap_or_default();
        [r.title.as_str(), repo, r.harness.as_str(), r.model.as_deref().unwrap_or_default(), account.as_str(), task.map(|t| t.prompt.as_str()).unwrap_or_default(), r.status.as_str()]
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

    /// The agent list (T-25): the visible agents grouped by repository, the most recently active
    /// repository and agent first. An agent that is working counts as active now.
    pub fn groups(&self) -> Vec<(String, Vec<&Run>)> {
        let recent = |r: &Run| if r.active() { i64::MAX } else { r.ended_ms.unwrap_or(r.created_ms) };
        let mut groups: Vec<(String, Vec<&Run>)> = Vec::new();
        for r in self.visible() {
            let repo = self.state.task(&r.task_id).map(|t| t.repo_root.clone()).unwrap_or_default();
            match groups.iter_mut().find(|g| g.0 == repo) {
                Some(g) => g.1.push(r),
                None => groups.push((repo, vec![r])),
            }
        }
        for g in groups.iter_mut() {
            g.1.sort_by(|a, b| recent(b).cmp(&recent(a)).then(b.created_ms.cmp(&a.created_ms)).then(b.id.cmp(&a.id)));
        }
        groups.sort_by(|a, b| recent(b.1[0]).cmp(&recent(a.1[0])).then(b.1[0].created_ms.cmp(&a.1[0].created_ms)).then(a.0.cmp(&b.0)));
        groups
    }

    /// Agent ids in the list's order (`J`/`K` walk it).
    pub fn list_ids(&self) -> Vec<String> {
        self.groups().into_iter().flat_map(|g| g.1.into_iter().map(|r| r.id.clone())).collect()
    }

    /// The list shows beside the grid from 100 columns (and 30 rows, below which the compact
    /// layout keeps the room), unless `L` hid it.
    pub fn list_shown(&self) -> bool {
        !self.list_hidden && !self.compact()
    }

    /// Below 100×30 one focused tile and a compact list take the screen (T-09).
    pub fn compact(&self) -> bool {
        self.size.0 < 100 || self.size.1 < 30
    }

    /// Picks an agent: it takes focus and its conversation opens beside the grid.
    pub fn pick(&mut self, id: &str) {
        if let Some(i) = self.index_of(id) {
            if self.focus.as_deref() != Some(id) {
                self.conv_scroll = 0;
            }
            self.focus_index(i);
            self.picked = true;
            self.dirty = true;
        }
    }

    /// `J`/`K`: the next or previous agent in the list; the first press picks the first (or last).
    fn pick_step(&mut self, delta: i32) {
        let ids = self.list_ids();
        if ids.is_empty() {
            return;
        }
        let n = ids.len() as i32;
        let at = self.focus.as_deref().and_then(|f| ids.iter().position(|i| i == f)).filter(|_| self.picked);
        let next = match at {
            Some(i) => (i as i32 + delta).rem_euclid(n),
            None if delta > 0 => 0,
            None => n - 1,
        };
        let id = ids[next as usize].clone();
        self.pick(&id);
    }

    fn scroll_conv(&mut self, delta: i64) {
        self.conv_scroll = (self.conv_scroll as i64 + delta).max(0) as usize;
        self.dirty = true;
    }

    /// The grid's room in cells: the body beside the list and the picked agent's conversation.
    pub fn grid_room(&self) -> (u16, u16) {
        let (w, h) = self.size;
        if self.compact() {
            return (w, h.saturating_sub(2));
        }
        let (list_w, conv_w) = side_widths(w, self.list_shown(), self.picked && self.focus.is_some());
        (w.saturating_sub(list_w + conv_w), h.saturating_sub(2))
    }

    /// Agents per page (T-37): up to 16, fewer when the grid has no room for that many tiles.
    pub fn page_size(&self) -> usize {
        let (w, h) = self.grid_room();
        let cols = (w / MIN_TILE_W).max(1) as usize;
        let rows = (h / MIN_TILE_H).max(1) as usize;
        (cols * rows).clamp(1, PAGE)
    }

    /// The grid's shape for `n` tiles in the room it has (T-37).
    pub fn grid_shape(&self, n: usize) -> (usize, usize) {
        let max_cols = (self.grid_room().0 / MIN_TILE_W).max(1) as usize;
        let (r, c) = shape(n);
        if c <= max_cols { (r, c) } else { (n.div_ceil(max_cols).max(1), max_cols) }
    }

    pub fn pages(&self) -> usize {
        self.visible().len().div_ceil(self.page_size()).max(1)
    }

    /// The agents on the current page (at most nine).
    pub fn page_agents(&self) -> Vec<&Run> {
        let ps = self.page_size();
        self.visible().into_iter().skip(self.page * ps).take(ps).collect()
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
        self.page = i / self.page_size();
        self.dirty = true;
        self.ensure_history();
    }

    /// Keeps focus on the same agent after the list changed (new agents, filter changes).
    pub fn settle_focus(&mut self) {
        let n = self.visible().len();
        let ps = self.page_size();
        if n == 0 {
            self.focus = None;
            self.page = 0;
            return;
        }
        match self.focus.clone().and_then(|f| self.index_of(&f)) {
            Some(i) => self.page = i / ps,
            None => {
                let i = (self.page * ps).min(n - 1);
                self.focus_index(i);
            }
        }
        self.page = self.page.min(self.pages() - 1);
    }

    fn move_focus(&mut self, dx: i32, dy: i32) {
        let ps = self.page_size();
        let shape = |n: usize| self.grid_shape(n);
        let Some(i) = self.focus.clone().and_then(|f| self.index_of(&f)) else {
            self.focus_index(self.page * ps);
            return;
        };
        let n = self.visible().len();
        let (page, slot) = (i / ps, i % ps);
        let on_page = |p: usize| n.saturating_sub(p * ps).min(ps);
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
                    Some((page - 1) * ps + (row as usize) * pc + pc - 1)
                }
            } else if c >= cols as i32 {
                // Past the right edge: the next page's same row (or its last agent).
                if on_page(page + 1) == 0 {
                    None
                } else {
                    let (nr, nc) = shape(on_page(page + 1));
                    Some(((page + 1) * ps + (row as usize).min(nr - 1) * nc).min(n - 1))
                }
            } else {
                Some(page * ps + (row * cols as i32 + c) as usize)
            }
        } else {
            let r = row + dy;
            let t = page * ps + (r.max(0) as usize) * cols + col as usize;
            if r >= 0 && t < page * ps + on_page(page) { Some(t) } else { None }
        };
        if let Some(t) = target {
            if t < n {
                self.focus_index(t);
            }
        }
    }

    fn change_page(&mut self, delta: i32) {
        let ps = self.page_size();
        let pages = self.pages() as i32;
        let next = (self.page as i32 + delta).clamp(0, pages - 1) as usize;
        if next == self.page {
            return;
        }
        let slot = self.focus.clone().and_then(|f| self.index_of(&f)).map(|i| i % ps).unwrap_or(0);
        let n = self.visible().len();
        self.focus_index((next * ps + slot).min(n.saturating_sub(1)));
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
        // The periodic audio question redraws only when its answer changes something.
        if !matches!(&msg, Msg::Reply { id, .. } if matches!(self.pending.get(id), Some(Pending::AudioGet))) {
            self.dirty = true;
        }
        match msg {
            Msg::Connected => {
                self.connected = true;
                if self.stopped {
                    self.stopped = false;
                    self.client.set_stopped(false);
                    self.say("The daemon is running again", false);
                }
                self.connect_generation += 1;
                self.state_inflight = false;
                self.request_state();
                self.phone_request();
                // What the last connection said about audio may no longer hold.
                self.audio.known = false;
                self.audio_inflight = false;
                self.request_audio();
            }
            Msg::Refused(why) => {
                self.connected = false;
                self.pending.clear();
                self.state_inflight = false;
                self.say(why, true);
            }
            Msg::Disconnected(why) => {
                self.connected = false;
                // Replies to requests on the old connection never come.
                self.pending.clear();
                self.pair_starting = 0;
                self.state_inflight = false;
                self.audio.known = false;
                self.audio_inflight = false;
                self.audio_due = None;
                self.audio_wanted = false;
                self.history_requested.retain(|r| self.feeds.get(r).is_some_and(|f| f.history_loaded));
                if self.stopped {
                    self.say("Agents and daemon stopped. Press r to start the daemon again.", false);
                } else {
                    self.say(format!("Reconnecting to overseerd ({why})…"), true);
                }
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

    fn request_audio(&mut self) {
        if self.audio_inflight {
            return;
        }
        self.audio_inflight = true;
        self.audio_due = None;
        self.request("audio.get", json!({}), Pending::AudioGet);
    }

    /// Timers: the debounced state reload and the audio settings. Returns true when something changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if let Some(due) = self.state_due {
            if now >= due && self.connected {
                self.request_state();
            }
        }
        if self.connected && self.audio_due.is_some_and(|due| now >= due) {
            self.request_audio();
        }
        if let Some((_, at, _)) = &self.notice {
            if now.duration_since(*at) > Duration::from_secs(6) {
                self.notice = None;
                changed = true;
            }
        }
        changed |= self.phone_tick(now);
        changed
    }

    /// Where a feed's paths are shortened from: its workspace and repository.
    fn locate_feed(state: &State, root: &str, feed: &mut Feed) {
        let unlocated = feed.root.is_none();
        if unlocated {
            feed.root = state.run(root).and_then(|r| state.workspace(&r.workspace_id)).map(|w| w.path.clone());
        }
        if feed.repo.is_none() {
            feed.repo = state.run(root).and_then(|r| state.task(&r.task_id)).map(|t| (t.repo_root.clone(), t.repo_root.rsplit('/').next().unwrap_or_default().to_string()));
        }
        if unlocated && feed.root.is_some() {
            feed.relocate();
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
        if phone::is_phone_event(&kind) {
            self.on_phone_event(&kind, &ev);
        }
        if kind == "daemon_stopping" {
            // Stopped from here or from VS Code: do not start it again behind the user's back.
            self.stopped = true;
            self.client.set_stopped(true);
        }
        if !run_id.is_empty() {
            let root = self.state.root_of(&run_id);
            let child = if root != run_id { Some(self.state.run(&run_id).map(|r| r.title.clone()).unwrap_or_else(|| "sub-agent".into())) } else { None };
            let feed = self.feeds.entry(root.clone()).or_default();
            Self::locate_feed(&self.state, &root, feed);
            feed.add(&ev, child.as_deref());
            self.last_event.insert(root, Instant::now());
        }
        // The review follows marks and rejections made elsewhere (VS Code, the phone).
        if self.mode == Mode::Changes && matches!(kind.as_str(), "review_mark" | "review_reject") && self.state.root_of(&run_id) == self.changes.run && !self.changes.loading {
            if kind == "review_reject" { self.load_diff() } else { self.load_file_diff() }
        }
        // The conversation with Overseer follows its own events while it is open.
        if matches!(self.mode, Mode::Overseer) && matches!(kind.as_str(), "overseer_message" | "proposal" | "proposal_answered" | "overseer_level" | "overseer_session") {
            self.request("overseer.session", json!({}), Pending::OverseerSession);
        }
        // Statuses, turns and new runs come from `state`, reloaded like VS Code does.
        if (matches!(kind.as_str(), "status" | "turn_started" | "turn_done" | "permission" | "permission_answered" | "child" | "child_reparented" | "task_created" | "workspace_removed" | "reattached" | "profile" | "review_seen" | "merge_back")
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
                            // Someone needs you: one signal (T-24). The daemon's cue when its latest
                            // answer says it plays; in every other case the bell, in this same pass.
                            if !self.audio.plays() {
                                self.bell = true;
                            }
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
                        if !self.accounts_read {
                            self.accounts_read = true;
                            let ids: Vec<String> = self.state.profiles.iter().map(|p| p.id.clone()).collect();
                            for id in ids {
                                self.request("profile.status", json!({ "id": id }), Pending::ProfileStatus(id.clone()));
                            }
                        }
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
            (Pending::OverseerSession, Ok(v)) => {
                self.overseer = v;
                self.dirty = true;
            }
            (Pending::OverseerSession, Err(e)) => self.say(format!("Overseer: {e}"), true),
            (Pending::OverseerSend, Ok(_)) => self.request("overseer.session", json!({}), Pending::OverseerSession),
            (Pending::OverseerSend, Err(e)) => self.say(format!("Overseer: {e}"), true),
            (Pending::OverseerAnswer, Ok(v)) => {
                self.say(v["result"].as_str().unwrap_or("answered").to_string(), false);
                self.request("overseer.session", json!({}), Pending::OverseerSession);
            }
            (Pending::OverseerAnswer, Err(e)) => self.say(format!("Overseer: {e}"), true),
            (Pending::AudioGet, Ok(v)) => {
                self.audio_inflight = false;
                self.audio_due = Some(Instant::now() + AUDIO_REFRESH);
                if self.audio.update(&v) {
                    self.dirty = true;
                }
                if std::mem::take(&mut self.audio_wanted) {
                    self.open_audio();
                }
            }
            (Pending::AudioGet, Err(e)) => {
                // A daemon without audio methods: nothing is known, so the bell keeps ringing.
                self.audio_inflight = false;
                self.audio_due = Some(Instant::now() + AUDIO_RETRY);
                let asked = std::mem::take(&mut self.audio_wanted) || matches!(self.mode, Mode::Audio | Mode::AudioImport);
                if self.audio.known || asked {
                    self.audio.known = false;
                    self.dirty = true;
                }
                if asked {
                    self.mode = Mode::Grid;
                    self.say(format!("Audio Mode is unavailable: {e}"), true);
                }
            }
            (Pending::AudioSet, Ok(v)) => {
                self.audio.update(&v);
                self.say(format!("Audio Mode {} · {}", if self.audio.enabled { "on" } else { "off" }, self.audio.track), false);
            }
            (Pending::AudioPreview, Ok(_)) => self.say("Preview queued by overseerd", false),
            (Pending::AudioVoices, Ok(v)) => {
                self.audio.voices = v.as_array().into_iter().flatten()
                    .filter_map(|item| item["name"].as_str().map(str::to_string)).collect();
                self.dirty = true;
            }
            (Pending::AudioVoices, Err(_)) => {
                // Reactor and private Commander still work if system speech is unavailable.
            }
            (Pending::AudioImport, Ok(_)) => {
                self.mode = Mode::Audio;
                self.request("audio.set", json!({"track": "commander"}), Pending::AudioSet);
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
                let keep = self.changes.comparison().map(|c| c.mode.clone());
                let opts: Vec<Value> = v["options"].as_array().cloned().unwrap_or_default();
                self.changes.options = opts.iter().map(|o| Comparison {
                    mode: o["mode"].as_str().unwrap_or_default().to_string(),
                    label: o["label"].as_str().unwrap_or("comparison").to_string(),
                    base: o["base"].as_str().map(str::to_string),
                    available: o["available"].as_bool().unwrap_or(false) && o["base"].is_string(),
                    detail: o["detail"].as_str().unwrap_or_default().to_string(),
                }).collect();
                // T-27: the comparison the daemon marks as the default ("Since task start"), else the first available.
                let default = opts.iter().position(|o| o["default"] == true && o["available"] == true && o["base"].is_string());
                let kept = keep.and_then(|m| self.changes.options.iter().position(|c| c.mode == m && c.available));
                match kept.or(default).or_else(|| self.changes.options.iter().position(|c| c.available)) {
                    Some(i) => {
                        self.changes.option = i;
                        self.load_diff();
                    }
                    None => {
                        self.changes.loading = false;
                        self.changes.error = Some("No comparison is available for this agent yet.".into());
                    }
                }
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
                let keep = self.changes.selected().map(|f| f.1);
                self.changes.files = v["changes"].as_array().cloned().unwrap_or_default().iter().map(|c| {
                    let path = c["path"].as_str().unwrap_or_default().to_string();
                    let (a, d) = counts.get(&path).copied().unwrap_or((0, 0));
                    (c["status"].as_str().unwrap_or("M").to_string(), path, a, d)
                }).collect();
                self.changes.file = keep.and_then(|k| self.changes.shown().iter().position(|f| f.1 == k)).unwrap_or(0);
                self.changes.error = None;
                self.load_file_diff();
            }
            (Pending::Hunks { run, path }, Ok(v)) => {
                if self.changes.run != run || self.changes.selected().map(|f| f.1) != Some(path.clone()) {
                    return;
                }
                let strings = |x: &Value| -> Vec<String> { x.as_array().map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect()).unwrap_or_default() };
                self.changes.hunks = v["hunks"].as_array().cloned().unwrap_or_default().iter().map(|h| Hunk {
                    key: h["key"].as_str().unwrap_or_default().to_string(),
                    base_start: h["base_start"].as_u64().unwrap_or(0) as usize,
                    base_lines: strings(&h["base_lines"]),
                    modified_start: h["modified_start"].as_u64().unwrap_or(0) as usize,
                    modified_lines: strings(&h["modified_lines"]),
                    reviewed: h["reviewed"] == true,
                }).collect();
                self.changes.change = self.changes.change.min(self.changes.hunks.len().saturating_sub(1));
                if v["shown"] == false {
                    self.changes.diff = vec![format!("({})", v["why"].as_str().filter(|w| !w.is_empty()).unwrap_or("not shown as text"))];
                    self.changes.hunk_at.clear();
                    self.changes.loading = false;
                    return;
                }
                let ws = self.changes.workspace.clone();
                self.request("workspace.file", json!({ "workspace_id": ws, "path": path }), Pending::FileText { run, path });
            }
            (Pending::FileText { run, path }, Ok(v)) => {
                if self.changes.run != run || self.changes.selected().map(|f| f.1) != Some(path.clone()) {
                    return;
                }
                self.changes.loading = false;
                let text = v["now"]["text"].as_str().map(str::to_string);
                // T-39: lines the owner wrote in the editor just now are theirs.
                if let Some((p, before)) = self.changes.editing.take() {
                    if p == path {
                        let mine = owner_lines(&before, text.as_deref().unwrap_or_default());
                        self.changes.mine.entry(p).or_default().extend(mine);
                    }
                }
                if self.changes.unchanged {
                    self.changes.diff = match (&text, v["now"]["note"].as_str()) {
                        (Some(t), _) => t.lines().take(DIFF_LINES).map(|l| format!(" {l}")).collect(),
                        (None, Some(note)) => vec![format!("({note})")],
                        (None, None) => vec!["(this file is not on disk)".into()],
                    };
                    self.changes.hunk_at.clear();
                } else {
                    self.render_hunks(text.as_deref().unwrap_or_default());
                }
            }
            (Pending::Tree { run }, Ok(v)) => {
                if self.changes.run != run {
                    return;
                }
                self.changes.all_loading = self.changes.all_loading.saturating_sub(1);
                for e in v["entries"].as_array().cloned().unwrap_or_default() {
                    let path = e["path"].as_str().unwrap_or_default().to_string();
                    if e["dir"] == true && e["symlink"] != true {
                        // Folders are listed in turn, up to a bound (a very large worktree is cut short).
                        if self.changes.all.len() < ALL_FILES_MAX {
                            self.changes.all_loading += 1;
                            let ws = self.changes.workspace.clone();
                            self.request("workspace.tree", json!({ "workspace_id": ws, "dir": path }), Pending::Tree { run: run.clone() });
                        }
                    } else if self.changes.all.len() < ALL_FILES_MAX && !self.changes.all.contains(&path) {
                        self.changes.all.push(path);
                    }
                }
                if self.changes.all_loading == 0 {
                    self.changes.all.sort();
                    self.changes.file = 0;
                    self.load_file_diff();
                }
            }
            (Pending::Accept { run, path, rest }, Ok(_)) => {
                if let Some(next) = rest.first().cloned() {
                    self.send_accept(&run, &path, next, rest[1..].to_vec());
                } else {
                    self.say(format!("Accepted in {path}"), false);
                    self.load_file_diff();
                }
            }
            (Pending::Reject { run, path, rest }, Ok(_)) => {
                if let Some(next) = rest.first().cloned() {
                    self.send_reject(&run, &path, next, rest[1..].to_vec());
                } else {
                    self.say(format!("Rejected in {path}: the lines from before are back"), false);
                    self.load_diff();
                }
            }
            (Pending::Accept { .. } | Pending::Reject { .. }, Err(e)) => {
                // The daemon's conflict check: the agent changed it meanwhile.
                self.say(e, true);
                self.load_diff();
            }
            (Pending::Hunks { .. } | Pending::FileText { .. } | Pending::Tree { .. }, Err(e)) => {
                self.changes.loading = false;
                self.changes.all_loading = 0;
                self.changes.error = Some(e);
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
                    edit: false,
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
            (Pending::PrPlan { run }, Ok(plan)) => {
                if plan["ok"] != true {
                    self.say(format!("Open PR is unavailable: {}", plan["reason"].as_str().unwrap_or("unknown reason")), true);
                    return;
                }
                let n = plan["uncommitted"].as_array().map(|a| a.len()).unwrap_or(0);
                let commit = if n > 0 { format!("commit {n} worktree file{}, ", if n == 1 { "" } else { "s" }) } else { String::new() };
                let text = format!(
                    "Open a pull request on {}/{}: {} → {}? This will {commit}push {} to {} with your Git credentials and create it with gh. Nothing is merged.",
                    plan["owner"].as_str().unwrap_or_default(), plan["repo"].as_str().unwrap_or_default(), plan["branch"].as_str().unwrap_or_default(),
                    plan["target"].as_str().unwrap_or_default(), plan["branch"].as_str().unwrap_or_default(), plan["remote"].as_str().unwrap_or("origin"));
                self.pr_plan = Some(plan);
                self.mode = Mode::Confirm(Confirm::OpenPr { run, text });
            }
            (Pending::PrPrepare { run, plan }, Ok(prep)) => {
                let ws = self.state.run(&run).and_then(|r| self.state.workspace(&r.workspace_id)).map(|w| w.path.clone()).unwrap_or_default();
                let body = pr_body(&plan, &prep);
                self.say("Pushing and opening the pull request…", false);
                self.job(Pending::PrPublish { run }, move || publish_pr(&ws, &plan, &body));
            }
            (Pending::PrPublish { run }, Ok(pr)) => {
                let url = pr["url"].as_str().unwrap_or_default().to_string();
                let number = pr["number"].as_i64().unwrap_or(0);
                if let Some(ws) = self.state.run(&run).map(|r| r.workspace_id.clone()) {
                    self.request("workspace.pr_opened", json!({ "workspace_id": ws, "url": url, "number": number }), Pending::PrOpened);
                }
                self.say(format!("Pull request #{number} is open: {url} (nothing was merged)"), false);
            }
            (Pending::PrOpened, Ok(_)) => {}
            (Pending::Seen, Ok(_)) => {}
            (Pending::StopAll, Ok(v)) => {
                let n = v["stopped"].as_array().map(|a| a.len()).unwrap_or(0);
                let left = v["remaining"].as_array().map(|a| a.len()).unwrap_or(0);
                self.say(format!("Stopped {n} agent{} and the daemon.{} Press r to start the daemon again.", if n == 1 { "" } else { "s" }, if left > 0 { format!(" {left} could not be stopped.") } else { String::new() }), left > 0);
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
            (why @ (Pending::PhoneStatus | Pending::PhoneSwitch { .. } | Pending::PairStart | Pending::PairCancel | Pending::PairConfirm { .. } | Pending::DeviceRevoke(_) | Pending::DeviceScope { .. } | Pending::PhoneNotifications(_)), result) => self.on_phone_reply(why, result),
            (_, Err(e)) => self.say(e, true),
        }
    }

    // ---------------------------------------------------------------- actions

    /// The terminal window title: counts that matter when the TUI is in another tab.
    pub fn window_title(&self) -> String {
        let all = self.state.agents();
        let needs = self.state.needs_you_count();
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

    /// Opening a finished agent's review clears its "to review" mark here, in VS Code and in the
    /// menu bar: the mark is the daemon's (T-26, as VS Code's markReviewed).
    fn mark_reviewed(&mut self, run_id: &str) {
        let root = self.state.root_of(run_id);
        let Some(run) = self.state.run(&root) else { return };
        if run.active() || !self.state.unreviewed(run, crate::model::now_ms()) {
            return;
        }
        let at = crate::model::now_ms();
        self.state.reviewed.insert(root.clone(), at);
        self.request("review.seen", json!({ "marks": { root: at } }), Pending::Seen);
    }

    fn open_changes(&mut self) {
        let Some(run) = self.focused().cloned() else { return };
        self.mark_reviewed(&run.id);
        let keep = std::mem::take(&mut self.changes.mine);
        self.changes = ChangesView { run: run.id.clone(), workspace: run.workspace_id.clone(), loading: true, mine: if self.changes.run == run.id { keep } else { HashMap::new() }, ..Default::default() };
        self.mode = Mode::Changes;
        self.request("comparison.options", json!({ "run_id": run.id }), Pending::Comparisons { run: run.id.clone() });
    }

    /// Reloads the comparisons (their bases move as turns start), then the files and the diff.
    fn reload_review(&mut self) {
        let run = self.changes.run.clone();
        self.changes.loading = true;
        self.request("comparison.options", json!({ "run_id": run }), Pending::Comparisons { run });
    }

    fn load_diff(&mut self) {
        let run = self.changes.run.clone();
        let ws = self.changes.workspace.clone();
        let Some(base) = self.changes.comparison().and_then(|c| c.base.clone()) else { return };
        self.changes.loading = true;
        self.request("workspace.diff", json!({ "workspace_id": ws, "base": base, "status": false }), Pending::Diff { run });
    }

    /// The selected file's changes from the daemon (`workspace.hunks`), or an unchanged file's
    /// contents (`workspace.file`).
    fn load_file_diff(&mut self) {
        let c = &mut self.changes;
        let Some((status, path, ..)) = c.selected() else {
            c.diff.clear();
            c.hunks.clear();
            c.hunk_at.clear();
            return;
        };
        // Another file starts at its first change; the same file (a refresh) keeps its place and
        // its lines until the new ones arrive.
        if c.path != path {
            c.path = path.clone();
            c.scroll = 0;
            c.diff.clear();
            c.hunks.clear();
            c.hunk_at.clear();
            c.change = 0;
        }
        let (run, ws) = (c.run.clone(), c.workspace.clone());
        c.unchanged = status.is_empty();
        c.loading = true;
        if c.unchanged {
            self.request("workspace.file", json!({ "workspace_id": ws, "path": path }), Pending::FileText { run, path });
            return;
        }
        let Some(base) = c.comparison().and_then(|x| x.base.clone()) else { return };
        self.request("workspace.hunks", json!({ "workspace_id": ws, "path": path, "base": base, "run_id": run }), Pending::Hunks { run, path });
    }

    /// Draws the changes as a diff: each change's header, two lines around it from the working
    /// copy, the comparison's lines (−) and the working copy's (+).
    fn render_hunks(&mut self, now: &str) {
        let lines: Vec<&str> = now.split('\n').collect();
        let c = &mut self.changes;
        c.now = crate::app::split_lines(now);
        c.diff.clear();
        c.hunk_at.clear();
        for h in &c.hunks {
            c.hunk_at.push(c.diff.len());
            c.diff.push(format!("@@ -{},{} +{},{} @@", h.base_start, h.base_lines.len(), h.modified_start, h.modified_lines.len()));
            // The working copy's lines before and after the change (1-based; a removal sits after its line).
            let (first, after) = if h.modified_lines.is_empty() { (h.modified_start + 1, h.modified_start + 1) } else { (h.modified_start, h.modified_start + h.modified_lines.len()) };
            for n in first.saturating_sub(CONTEXT).max(1)..first {
                if let Some(l) = lines.get(n - 1) {
                    c.diff.push(format!(" {l}"));
                }
            }
            c.diff.extend(h.base_lines.iter().map(|l| format!("-{l}")));
            c.diff.extend(h.modified_lines.iter().map(|l| format!("+{l}")));
            for n in after..after + CONTEXT {
                if let Some(l) = lines.get(n - 1).filter(|_| n <= lines.len()) {
                    c.diff.push(format!(" {l}"));
                }
            }
        }
        if c.hunks.is_empty() {
            c.diff.push("(no textual change)".into());
        }
        c.change = c.change.min(c.hunks.len().saturating_sub(1));
    }

    fn send_accept(&mut self, run: &str, path: &str, h: Hunk, rest: Vec<Hunk>) {
        let mut p = json!({ "run_id": run, "path": path, "key": h.key, "modified_start": h.modified_start, "modified_lines": h.modified_lines, "base_lines": h.base_lines });
        if h.modified_lines.is_empty() {
            // A change that only removes lines is checked at the line it sits after.
            if let Some(anchor) = h.modified_start.checked_sub(1).and_then(|i| self.changes.now.get(i)) {
                p["anchor"] = json!(anchor);
            }
        }
        self.request("review.accept", p, Pending::Accept { run: run.to_string(), path: path.to_string(), rest });
    }

    fn send_reject(&mut self, run: &str, path: &str, key: String, rest: Vec<String>) {
        let ws = self.changes.workspace.clone();
        let Some(base) = self.changes.comparison().and_then(|c| c.base.clone()) else { return };
        self.request("review.reject", json!({ "workspace_id": ws, "path": path, "base": base, "key": key }), Pending::Reject { run: run.to_string(), path: path.to_string(), rest });
    }

    /// `1`, `2`, `3` and `c`: another comparison; one that is not available says why (T-27).
    fn choose_comparison(&mut self, i: usize) {
        let Some(c) = self.changes.options.get(i).cloned() else { return };
        if !c.available {
            self.say(format!("{} is not available: {}", c.label, if c.detail.is_empty() { "the daemon has no base for it" } else { &c.detail }), true);
            return;
        }
        self.changes.option = i;
        self.changes.files.clear();
        self.changes.hunks.clear();
        self.changes.diff.clear();
        self.changes.path.clear();
        self.load_diff();
    }

    fn review_key_comparison(&mut self, n: usize) {
        let (mode, label) = REVIEW_KEYS[n];
        match self.changes.options.iter().position(|c| c.mode == mode) {
            Some(i) => self.choose_comparison(i),
            None => self.say(format!("{label} is not available: this daemon does not offer it yet"), true),
        }
    }

    /// Moves to change `i` of the file and scrolls it into view.
    fn go_to_change(&mut self, i: usize) {
        let c = &mut self.changes;
        if c.hunks.is_empty() {
            return;
        }
        c.change = i.min(c.hunks.len() - 1);
        c.scroll = c.hunk_at.get(c.change).copied().unwrap_or(0).saturating_sub(2);
    }

    /// `e` (T-39): the file at the current change in the owner's `$EDITOR` (else `vi`), with the
    /// TUI suspended; the review refreshes when the editor exits.
    fn edit_in_editor(&mut self) {
        let Some((_, path, ..)) = self.changes.selected() else { return };
        let root = self.state.workspace(&self.changes.workspace).map(|w| w.path.clone()).unwrap_or_else(|| self.changes.root.clone());
        if root.is_empty() {
            return;
        }
        let line = self.changes.hunks.get(self.changes.change).map(|h| h.modified_start.max(1)).unwrap_or(1);
        let before = std::fs::read_to_string(std::path::Path::new(&root).join(&path)).unwrap_or_default();
        self.changes.editing = Some((path.clone(), before));
        let editor = std::env::var("EDITOR").ok().filter(|e| !e.trim().is_empty()).unwrap_or_else(|| "vi".into());
        let mut words = editor.split_whitespace().map(str::to_string);
        let program = words.next().unwrap_or_else(|| "vi".into());
        let mut args: Vec<String> = words.collect();
        args.push(format!("+{line}"));
        args.push(std::path::Path::new(&root).join(&path).display().to_string());
        self.exec = Some(Exec { title: format!("Editing {path}"), program, args, env: Vec::new(), edit: true });
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

    /// `S`: the panel opens on what the daemon said; without an answer yet it waits for one, and
    /// a daemon without audio methods changes nothing.
    /// The conversation with Overseer (AC-199): one key opens it from the grid.
    fn open_overseer(&mut self) {
        self.mode = Mode::Overseer;
        self.overseer_scroll = 0;
        self.request("overseer.session", json!({}), Pending::OverseerSession);
    }

    /// Keys in the conversation: type and Enter sends; ctrl+y / ctrl+n answer the first proposal
    /// that waits; j/k scroll when nothing is typed; esc closes (the draft is kept).
    fn overseer_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.mode = Mode::Grid,
            KeyCode::Char('y') | KeyCode::Char('n') if ctrl => {
                let yes = k.code == KeyCode::Char('y');
                let open = self.overseer["proposals"].as_array().and_then(|p| p.iter().find(|x| x["state"] == "open")).map(|p| p["id"].as_str().unwrap_or("").to_string());
                match open {
                    Some(id) => self.request("overseer.answer", json!({"id": id, "yes": yes, "surface": "tui", "by": "owner"}), Pending::OverseerAnswer),
                    None => self.say("No proposal waits for you", false),
                }
            }
            KeyCode::Char('u') if ctrl => self.overseer_draft.clear(),
            KeyCode::Char('r') if ctrl => self.request("overseer.session", json!({}), Pending::OverseerSession),
            KeyCode::Enter if k.modifiers.contains(KeyModifiers::ALT) || k.modifiers.contains(KeyModifiers::SHIFT) => self.overseer_draft.push('\n'),
            KeyCode::Enter => {
                let text = self.overseer_draft.trim().to_string();
                if text.is_empty() {
                    return;
                }
                self.overseer_draft.clear();
                self.request("overseer.send", json!({"text": text, "surface": "tui"}), Pending::OverseerSend);
            }
            KeyCode::Backspace => {
                self.overseer_draft.pop();
            }
            KeyCode::Down | KeyCode::Char('j') if self.overseer_draft.is_empty() => self.overseer_scroll = self.overseer_scroll.saturating_sub(1),
            KeyCode::Up | KeyCode::Char('k') if self.overseer_draft.is_empty() => self.overseer_scroll += 1,
            KeyCode::Char(c) if !c.is_control() => self.overseer_draft.push(c),
            _ => {}
        }
        self.dirty = true;
    }

    fn open_audio(&mut self) {
        if !self.audio.known {
            self.audio_wanted = true;
            self.audio_inflight = false;
            self.request_audio();
            return;
        }
        self.mode = Mode::Audio;
        self.request_audio();
        self.request("audio.voices", json!({}), Pending::AudioVoices);
    }

    fn audio_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc | KeyCode::Char('S') => self.mode = Mode::Grid,
            KeyCode::Char(' ') | KeyCode::Char('e') =>
                self.request("audio.set", json!({"enabled": !self.audio.enabled}), Pending::AudioSet),
            KeyCode::Char('1') => self.request("audio.set", json!({"track": "reactor"}), Pending::AudioSet),
            KeyCode::Char('2') => self.request("audio.set", json!({"track": "system"}), Pending::AudioSet),
            KeyCode::Char('3') => self.request("audio.set", json!({"track": "commander"}), Pending::AudioSet),
            KeyCode::Tab | KeyCode::Right =>
                self.audio.preview = (self.audio.preview + 1) % AudioSettings::CORE_KEYS.len(),
            KeyCode::BackTab | KeyCode::Left =>
                self.audio.preview = (self.audio.preview + AudioSettings::CORE_KEYS.len() - 1) % AudioSettings::CORE_KEYS.len(),
            KeyCode::Char('p') => self.request("audio.preview", json!({"key": self.audio.preview_key()}), Pending::AudioPreview),
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if self.audio.voices.is_empty() {
                    self.say("No installed macOS voices are available", true);
                } else {
                    let current = self.audio.voices.iter().position(|v| v == &self.audio.voice);
                    let index = if k.code == KeyCode::Char('V') {
                        current.unwrap_or(0).wrapping_add(self.audio.voices.len() - 1) % self.audio.voices.len()
                    } else {
                        (current.map(|i| i + 1).unwrap_or(0)) % self.audio.voices.len()
                    };
                    self.request("audio.set", json!({"track": "system", "voice": self.audio.voices[index]}), Pending::AudioSet);
                }
            }
            KeyCode::Char('i') => self.mode = Mode::AudioImport,
            KeyCode::Char('r') => self.request_audio(),
            _ => self.dirty = false,
        }
    }

    fn audio_import_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Audio,
            KeyCode::Enter => {
                let path = self.audio.import_path.trim();
                if path.is_empty() {
                    self.say("Enter the private Commander folder path", true);
                } else {
                    self.request("audio.import_commander", json!({"path": path}), Pending::AudioImport);
                }
            }
            KeyCode::Backspace => { self.audio.import_path.pop(); }
            KeyCode::Char(c) if !c.is_control() && self.audio.import_path.chars().count() < MAX_AUDIO_IMPORT_PATH => self.audio.import_path.push(c),
            _ => self.dirty = false,
        }
    }

    fn open_accounts(&mut self) {
        self.mode = Mode::Accounts;
        self.request("account.list", json!({}), Pending::AccountList);
    }

    /// Called by the event loop after a suspended program (a sign-in) finished.
    pub fn after_exec(&mut self, result: Result<i32, String>) {
        if self.mode == Mode::Changes && self.changes.editing.is_some() {
            // T-39: back from the owner's editor; the review shows the file as it is now.
            match result {
                Ok(_) => self.say("Back from your editor; the review shows your edits", false),
                Err(e) => {
                    self.changes.editing = None;
                    self.say(format!("Could not open your editor: {e}"), true);
                }
            }
            self.load_diff();
            self.dirty = true;
            return;
        }
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
        let n = self.changes.shown().len();
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let page = (self.size.1 as usize).saturating_sub(8);
        match k.code {
            KeyCode::Char('r') if ctrl => self.reload_review(),
            KeyCode::Esc | KeyCode::Char('v') | KeyCode::Char('q') => self.mode = Mode::Grid,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => {
                self.changes.file = (self.changes.file + 1) % n;
                self.load_file_diff();
            }
            KeyCode::Up | KeyCode::Char('k') if n > 0 => {
                self.changes.file = (self.changes.file + n - 1) % n;
                self.load_file_diff();
            }
            KeyCode::Char('J') | KeyCode::PageDown => self.changes.scroll = (self.changes.scroll + page).min(self.changes.diff.len().saturating_sub(1)),
            KeyCode::Char('K') | KeyCode::PageUp => self.changes.scroll = self.changes.scroll.saturating_sub(page),
            KeyCode::Char('n') => self.go_to_change(self.changes.change + 1),
            KeyCode::Char('p') => self.go_to_change(self.changes.change.saturating_sub(1)),
            KeyCode::Char('c') if !self.changes.options.is_empty() => {
                let len = self.changes.options.len();
                if let Some(i) = (1..=len).map(|d| (self.changes.option + d) % len).find(|&i| self.changes.options[i].available) {
                    self.choose_comparison(i);
                }
            }
            KeyCode::Char(c @ '1'..='3') => self.review_key_comparison(c as usize - '1' as usize),
            KeyCode::Char('t') => {
                self.changes.all_files = !self.changes.all_files;
                self.changes.file = 0;
                if self.changes.all_files && self.changes.all.is_empty() && self.changes.all_loading == 0 {
                    let (run, ws) = (self.changes.run.clone(), self.changes.workspace.clone());
                    self.changes.all_loading = 1;
                    self.request("workspace.tree", json!({ "workspace_id": ws, "dir": "" }), Pending::Tree { run });
                } else {
                    self.load_file_diff();
                }
            }
            KeyCode::Char(c @ ('a' | 'A')) => {
                let Some((_, path, ..)) = self.changes.selected() else { return };
                let todo: Vec<Hunk> = if c == 'a' {
                    self.changes.hunks.get(self.changes.change).filter(|h| !h.reviewed).cloned().into_iter().collect()
                } else {
                    self.changes.hunks.iter().filter(|h| !h.reviewed).cloned().collect()
                };
                match todo.first().cloned() {
                    Some(first) => {
                        let run = self.changes.run.clone();
                        self.send_accept(&run, &path, first, todo[1..].to_vec());
                    }
                    None if self.changes.hunks.is_empty() => self.say("No change to accept here", false),
                    None => self.say("Already accepted", false),
                }
            }
            KeyCode::Char(c @ ('r' | 'R')) => {
                let Some((_, path, ..)) = self.changes.selected() else { return };
                let todo: Vec<Hunk> = if c == 'r' { self.changes.hunks.get(self.changes.change).cloned().into_iter().collect() } else { self.changes.hunks.clone() };
                if todo.is_empty() {
                    self.say("No change to reject here", false);
                    return;
                }
                let back: usize = todo.iter().map(|h| h.base_lines.len()).sum();
                let gone: usize = todo.iter().map(|h| h.modified_lines.len()).sum();
                let lines = |n: usize| format!("{n} line{}", if n == 1 { "" } else { "s" });
                let what = if c == 'r' { format!("this change in {path}") } else { format!("all {} change{} in {path}", todo.len(), if todo.len() == 1 { "" } else { "s" }) };
                let text = format!("Reject {what}? {} go{} back to what was there before; {} the agent wrote {} removed.", lines(back), if back == 1 { "es" } else { "" }, lines(gone), if gone == 1 { "is" } else { "are" });
                self.mode = Mode::Confirm(Confirm::Reject { path, keys: todo.into_iter().map(|h| h.key).collect(), text });
            }
            KeyCode::Char('e') => self.edit_in_editor(),
            _ => {}
        }
    }

    // ---------------------------------------------------------------- input

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        let inside = |(_, x, y, w, h): &(String, u16, u16, u16, u16)| m.column >= *x && m.column < x + w && m.row >= *y && m.row < y + h;
        if let MouseEventKind::Down(MouseButton::Left) = m.kind {
            if let Some((id, ..)) = self.list_hit.iter().find(|r| inside(r)).cloned() {
                self.pick(&id);
                return;
            }
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
        } else if self.picked && self.mode == Mode::Grid {
            match m.kind {
                MouseEventKind::ScrollUp => self.scroll_conv(3),
                MouseEventKind::ScrollDown => self.scroll_conv(-3),
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
            Mode::Confirm(c @ (Confirm::PhoneOff { .. } | Confirm::PhoneOnAndPair | Confirm::Revoke { .. } | Confirm::Pair { .. })) => {
                self.phone_confirm(&c, k);
            }
            Mode::Confirm(Confirm::Reject { path, keys, .. }) => {
                self.mode = Mode::Changes;
                if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    if let Some(first) = keys.first().cloned() {
                        let run = self.changes.run.clone();
                        self.send_reject(&run, &path, first, keys[1..].to_vec());
                    }
                }
            }
            Mode::Confirm(c) => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.mode = Mode::Grid;
                    match c {
                        Confirm::Interrupt(run) => self.request("run.interrupt", json!({ "run_id": run }), Pending::Interrupt),
                        Confirm::Quit => self.leave(),
                        Confirm::MergePrepare { run, .. } => {
                            if let Some(ws) = self.state.run(&run).map(|r| r.workspace_id.clone()) {
                                self.say("Preparing merge back…", false);
                                self.request("workspace.merge_prepare", json!({ "workspace_id": ws, "handoff": true }), Pending::MergePrepare { run });
                            }
                        }
                        Confirm::OpenPr { run, .. } => {
                            if let (Some(ws), Some(plan)) = (self.state.run(&run).map(|r| r.workspace_id.clone()), self.pr_plan.take()) {
                                self.say("Committing and pushing…", false);
                                self.request("workspace.pr_prepare", json!({ "workspace_id": ws }), Pending::PrPrepare { run, plan });
                            }
                        }
                        Confirm::StopAll { .. } => {
                            self.stopped = true;
                            self.client.set_stopped(true);
                            self.request("daemon.stop_all", json!({}), Pending::StopAll);
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
                        // The phone questions are answered in `phone_confirm`; Reject above.
                        Confirm::Reject { .. } => {}
                        Confirm::PhoneOff { .. } | Confirm::PhoneOnAndPair | Confirm::Revoke { .. } | Confirm::Pair { .. } => {}
                    }
                }
                _ => self.mode = Mode::Grid,
            },
            Mode::Compose => self.compose_key(k),
            Mode::NewAgent => self.form_key(k),
            Mode::Changes => self.changes_key(k),
            Mode::Search => self.search_key(k),
            Mode::Accounts => self.accounts_key(k),
            Mode::Devices => self.devices_key(k),
            Mode::Pairing => self.pairing_key(k),
            Mode::Audio => self.audio_key(k),
            Mode::Overseer => self.overseer_key(k),
            Mode::AudioImport => self.audio_import_key(k),
            Mode::Grid | Mode::Zoom { .. } => self.nav_key(k),
        }
    }

    fn try_quit(&mut self) {
        if self.drafts.values().any(|d| !d.trim().is_empty()) && self.mode != Mode::Confirm(Confirm::Quit) {
            self.mode = Mode::Confirm(Confirm::Quit);
        } else {
            self.leave();
        }
    }

    /// Quits. A pairing code this terminal is showing is taken back first.
    fn leave(&mut self) {
        self.take_back_pairing();
        self.quit = true;
    }

    fn nav_key(&mut self, k: KeyEvent) {
        let zoom = matches!(self.mode, Mode::Zoom { .. });
        match k.code {
            KeyCode::Char('q') => self.try_quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Esc if zoom => self.mode = Mode::Grid,
            // The agent list (T-25): J/K pick the next or previous agent; Esc closes its conversation.
            KeyCode::Char('J') => self.pick_step(1),
            KeyCode::Char('K') => self.pick_step(-1),
            KeyCode::Char('L') => self.list_hidden = !self.list_hidden,
            KeyCode::Esc if self.picked => self.picked = false,
            // z, and g (T-38): the grid and the focused agent's full view, back on the same agent.
            KeyCode::Char('z') | KeyCode::Char('g') => self.mode = if zoom { Mode::Grid } else { Mode::Zoom { scroll: 0 } },
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
            KeyCode::Char('O') => self.toggle_phone_access(),
            KeyCode::Char('D') => self.open_devices(),
            KeyCode::Char('S') => self.open_audio(),
            KeyCode::Char('o') => self.open_overseer(),
            KeyCode::Char('P') => {
                if let Some(run) = self.focused().cloned() {
                    if run.active() {
                        self.say("Open PR waits until the agent is done (x interrupts it)", false);
                    } else {
                        let ws = run.workspace_id.clone();
                        self.request("workspace.pr_plan", json!({ "workspace_id": ws }), Pending::PrPlan { run: run.id.clone() });
                    }
                }
            }
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
            KeyCode::Char('r') if self.stopped => {
                self.client.set_stopped(false);
                self.say("Starting the daemon…", false);
            }
            KeyCode::Char('r') => self.request_state(),
            KeyCode::Char('X') => {
                let active: Vec<String> = self.state.agents().iter().filter(|r| r.active()).map(|r| short(&r.title, 30)).collect();
                let text = if active.is_empty() {
                    "Stop the Overseer daemon? No agents are running; worktrees and history are kept.".to_string()
                } else {
                    format!("Stop {} running agent{} ({}) and the daemon? Worktrees and history are kept.", active.len(), if active.len() == 1 { "" } else { "s" }, active.join(", "))
                };
                self.mode = Mode::Confirm(Confirm::StopAll { text });
            }
            // Zoom scrolling.
            KeyCode::Char('k') | KeyCode::Up if zoom => self.scroll(1),
            KeyCode::Char('j') | KeyCode::Down if zoom => self.scroll(-1),
            KeyCode::PageUp if zoom => self.scroll(self.size.1 as i64 - 4),
            KeyCode::PageDown if zoom => self.scroll(-(self.size.1 as i64 - 4)),
            KeyCode::Char('e') if zoom => self.expand_tools = !self.expand_tools,
            KeyCode::Home if zoom => self.mode = Mode::Zoom { scroll: usize::MAX / 2 },
            KeyCode::Char('G') | KeyCode::End if zoom => self.mode = Mode::Zoom { scroll: 0 },
            // The picked agent's conversation beside the grid: the same scrollback and tool details as zoom.
            KeyCode::PageUp if self.picked => self.scroll_conv(self.size.1 as i64 - 6),
            KeyCode::PageDown if self.picked => self.scroll_conv(-(self.size.1 as i64 - 6)),
            KeyCode::Home if self.picked => self.conv_scroll = usize::MAX / 2,
            KeyCode::End if self.picked => self.conv_scroll = 0,
            KeyCode::Char('e') if self.picked => self.expand_tools = !self.expand_tools,
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
                let ps = self.page_size();
                if slot < ps && self.page * ps + slot < self.visible().len() {
                    self.focus_index(self.page * ps + slot);
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
            Mode::AudioImport => {
                let remaining = MAX_AUDIO_IMPORT_PATH.saturating_sub(self.audio.import_path.chars().count());
                self.audio.import_path.extend(text.chars().take(remaining));
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

/// The pull request description, like VS Code's Open PR: the run, the task, commits and files.
pub fn pr_body(plan: &Value, prep: &Value) -> String {
    let mut out = format!("Opened by Overseer from run `{}`", plan["run_id"].as_str().unwrap_or_default());
    if let Some(h) = plan["harness"].as_str() {
        out.push_str(&format!(" ({h}{})", plan["model"].as_str().map(|m| format!(", {m}")).unwrap_or_default()));
    }
    out.push_str(".\n\n");
    if let Some(p) = plan["prompt"].as_str().filter(|p| !p.trim().is_empty()) {
        out.push_str("**Task**\n\n");
        for l in p.chars().take(1500).collect::<String>().lines() {
            out.push_str(&format!("> {l}\n"));
        }
        out.push('\n');
    }
    let commits: Vec<&str> = prep["commits"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    if !commits.is_empty() {
        out.push_str("**Commits**\n\n");
        for c in commits.iter().take(30) {
            out.push_str(&format!("- {c}\n"));
        }
        out.push('\n');
    }
    let files = prep["files"].as_array().cloned().unwrap_or_default();
    if !files.is_empty() {
        out.push_str(&format!("**Files changed** ({})\n\n", files.len()));
        for f in files.iter().take(50) {
            out.push_str(&format!("- `{}` {}\n", f["status"].as_str().unwrap_or("M"), f["path"].as_str().unwrap_or_default()));
        }
        out.push('\n');
    }
    out.push_str("_Review before merging. Overseer never merges automatically._");
    out
}

/// Pushes the branch with the user's own Git credentials and creates the pull request with
/// their GitHub CLI. No token passes through Overseer. Reuses an existing pull request.
fn publish_pr(ws: &str, plan: &Value, body: &str) -> Result<Value, String> {
    let s = |k: &str| plan[k].as_str().unwrap_or_default().to_string();
    let (remote, branch, target, repo) = (s("remote"), s("branch"), s("target"), format!("{}/{}", s("owner"), s("repo")));
    let push = std::process::Command::new("git").args(["-C", ws, "push", "--no-verify", &remote, &format!("HEAD:refs/heads/{branch}")]).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| e.to_string())?;
    if !push.status.success() {
        return Err(format!("git push failed: {}", String::from_utf8_lossy(&push.stderr).trim()));
    }
    let gh = std::env::var("OVERSEER_GH").unwrap_or_else(|_| "gh".into());
    let created = std::process::Command::new(&gh).args(["pr", "create", "--repo", &repo, "--head", &branch, "--base", &target, "--title", &s("title"), "--body", body]).output()
        .map_err(|e| format!("GitHub CLI (gh) not found ({e}); install it and run gh auth login, or use Open PR in VS Code"))?;
    let out = String::from_utf8_lossy(&created.stdout).to_string();
    let err = String::from_utf8_lossy(&created.stderr).to_string();
    let url = if created.status.success() {
        out.lines().rev().find(|l| l.starts_with("https://")).map(str::to_string)
    } else if err.contains("already exists") {
        // Reuse the open pull request for this branch.
        err.lines().chain(out.lines()).find_map(|l| l.split_whitespace().find(|w| w.starts_with("https://")).map(str::to_string))
    } else {
        return Err(format!("gh pr create failed: {}", err.trim()));
    };
    let url = url.ok_or_else(|| "gh did not report the pull request URL".to_string())?;
    let number = url.rsplit('/').next().and_then(|n| n.parse::<i64>().ok()).unwrap_or(0);
    Ok(json!({ "url": url, "number": number }))
}

/// Grid shape (rows, columns) for `n` agents on a page (T-37): 1, 1×2, 1×3, 2×2, 2×3, 3×3,
/// 3×4, 4×4.
pub fn shape(n: usize) -> (usize, usize) {
    match n {
        0 | 1 => (1, 1),
        2 => (1, 2),
        3 => (1, 3),
        4 => (2, 2),
        5 | 6 => (2, 3),
        7..=9 => (3, 3),
        10..=12 => (3, 4),
        _ => (4, 4),
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

/// The widths of the agent list and of the picked agent's conversation beside the grid
/// (T-25), for a terminal `width` columns wide; the grid takes the rest.
pub fn side_widths(width: u16, list: bool, conversation: bool) -> (u16, u16) {
    let list_w = if list { (width / 5).clamp(30, 42) } else { 0 };
    let conv_w = if conversation { (width.saturating_sub(list_w) * 2 / 5).clamp(40, 90) } else { 0 };
    (list_w, conv_w)
}

/// Lines in `after` that the owner wrote (T-39): what differs from `before` between their common
/// first and last lines.
pub fn owner_lines(before: &str, after: &str) -> HashSet<String> {
    let (b, a): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    let head = b.iter().zip(a.iter()).take_while(|(x, y)| x == y).count();
    let tail = b[head..].iter().rev().zip(a[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    a[head..a.len() - tail].iter().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect()
}

/// Lines as the daemon counts them (`\n`, `\r\n` or `\r`), for the checks it makes.
pub fn split_lines(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(str::to_string).collect()
}
