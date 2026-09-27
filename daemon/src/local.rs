//! Local models (Continuity): what this machine has and can run (AC-85), the memory budget and
//! what fits in it (AC-86), the catalogue of coding models (AC-87), and the guard that keeps any
//! model above the budget from being loaded, by any path (AC-140).
//!
//! Ollama is only ever addressed on loopback. Unknown stays unknown: a model whose size cannot be
//! measured or estimated is never treated as small.

use crate::sys::{self, gib, Memory, Pressure, GIB};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub const CATALOGUE: &str = include_str!("local_catalogue.json");
/// Compute buffers on top of the weights and the KV cache. 0.4 GiB was observed; kept conservative.
pub const OVERHEAD: u64 = GIB;
/// A model that only fits below this context is considered after every model that fits at it.
pub const COMFORTABLE_CONTEXT: u64 = 32768;
pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";

// ------------------------------------------------------------------ Ollama over loopback

/// The Ollama address: `OVERSEER_OLLAMA_URL` (tests) or the default. Anything but loopback is refused.
pub fn ollama_url() -> Result<String> {
    let url = std::env::var("OVERSEER_OLLAMA_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let rest = url.strip_prefix("http://").ok_or_else(|| anyhow!("Ollama is addressed over plain HTTP on loopback, not {url}"))?;
    let host = if let Some(v6) = rest.strip_prefix('[') { v6.split(']').next().unwrap_or_default() } else { rest.split([':', '/']).next().unwrap_or_default() };
    if !["127.0.0.1", "localhost", "::1"].contains(&host) {
        bail!("Ollama is only ever addressed on this machine (loopback), not {host}");
    }
    Ok(url.trim_end_matches('/').to_string())
}

fn agent(seconds: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(2)).timeout(Duration::from_secs(seconds)).build()
}

fn http(result: std::result::Result<ureq::Response, ureq::Error>) -> Result<Value> {
    match result {
        Ok(r) => Ok(serde_json::from_str(&r.into_string()?).unwrap_or(Value::Null)),
        Err(ureq::Error::Status(code, r)) => bail!("Ollama answered {code}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>()),
        Err(e) => bail!("Ollama is not answering: {e}"),
    }
}

pub fn get(path: &str, seconds: u64) -> Result<Value> {
    http(agent(seconds).get(&format!("{}{path}", ollama_url()?)).call())
}

pub fn post(path: &str, body: &Value, seconds: u64) -> Result<Value> {
    http(agent(seconds).post(&format!("{}{path}", ollama_url()?)).set("content-type", "application/json").send_string(&body.to_string()))
}

// ------------------------------------------------------------------ models

/// What the KV cache needs: the KV heads of every layer and the width of a key and a value.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Geometry {
    pub kv_heads_per_layer: Vec<u64>,
    pub key_length: u64,
    pub value_length: u64,
}

impl Geometry {
    /// Bytes of KV cache per token of context, with Ollama's default f16 cache.
    pub fn kv_bytes_per_token(&self) -> u64 {
        self.kv_heads_per_layer.iter().sum::<u64>() * (self.key_length + self.value_length) * 2
    }

    fn build(blocks: Option<u64>, kv: &Value, key: Option<u64>, value: Option<u64>, embedding: Option<u64>, heads: Option<u64>) -> Option<Self> {
        let per_layer: Vec<u64> = match kv {
            // Hybrid-attention models report one entry per layer, zero for layers without a cache.
            Value::Array(a) => a.iter().map(|x| x.as_u64().unwrap_or(0)).collect(),
            Value::Number(n) => vec![n.as_u64()?; blocks? as usize],
            _ => return None,
        };
        let width = || Some(embedding? / heads.filter(|h| *h > 0)?);
        let key_length = key.or_else(width)?;
        let value_length = value.or_else(width)?;
        (!per_layer.is_empty() && key_length > 0).then_some(Self { kv_heads_per_layer: per_layer, key_length, value_length })
    }

    /// From `/api/show`'s `model_info`, whose keys carry the architecture's name.
    pub fn from_model_info(info: &Value) -> Option<Self> {
        let arch = info["general.architecture"].as_str()?;
        let n = |key: &str| info[format!("{arch}.{key}")].as_u64();
        Self::build(n("block_count"), &info[format!("{arch}.attention.head_count_kv")], n("attention.key_length"), n("attention.value_length"), n("embedding_length"), n("attention.head_count"))
    }

    fn from_catalogue(g: &Value) -> Option<Self> {
        Self::build(g["block_count"].as_u64(), &g["head_count_kv"], g["key_length"].as_u64(), g["value_length"].as_u64(), None, None)
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Model {
    pub tag: String,
    /// The tag this one was derived from (its own when it is not derived); the catalogue's key.
    pub base: String,
    /// Bytes on disk.
    pub size: u64,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub parameters: Option<u64>,
    pub quantization: Option<String>,
    /// The longest context the model supports.
    pub max_context: Option<u64>,
    /// The context this tag is loaded with (`num_ctx` in its parameters), when it sets one.
    pub configured_context: Option<u64>,
    pub capabilities: Vec<String>,
    pub geometry: Option<Geometry>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Loaded {
    pub tag: String,
    /// Bytes in memory, as Ollama measures them.
    pub size: u64,
    pub size_vram: Option<u64>,
    pub context: Option<u64>,
    pub expires_at: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Ollama {
    /// Where the program was found, if it was.
    pub installed: Option<String>,
    pub version: Option<String>,
    pub running: bool,
    pub url: String,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Inventory {
    pub memory: Option<Memory>,
    pub memory_error: Option<String>,
    pub ollama: Ollama,
    pub models: Vec<Model>,
    pub loaded: Vec<Loaded>,
    pub models_dir: String,
    /// Free bytes where Ollama keeps its models.
    pub disk_free: Option<u64>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// `num_ctx` from the parameters text of `/api/show`.
pub fn configured_context(parameters: &str) -> Option<u64> {
    parameters.lines().find_map(|l| l.trim().strip_prefix("num_ctx")).and_then(|v| v.trim().parse().ok())
}

pub fn model_from(tag_entry: &Value, show: &Value) -> Model {
    let details = &tag_entry["details"];
    let tag = tag_entry["name"].as_str().unwrap_or_default().to_string();
    let info = &show["model_info"];
    let arch = info["general.architecture"].as_str().unwrap_or_default();
    let capabilities = if show["capabilities"].is_array() { strings(&show["capabilities"]) } else { strings(&tag_entry["capabilities"]) };
    Model {
        base: text(&details["parent_model"]).or_else(|| text(&show["details"]["parent_model"])).unwrap_or_else(|| tag.clone()),
        size: tag_entry["size"].as_u64().unwrap_or(0),
        family: text(&details["family"]),
        parameter_size: text(&details["parameter_size"]),
        parameters: info["general.parameter_count"].as_u64(),
        quantization: text(&details["quantization_level"]),
        max_context: info[format!("{arch}.context_length")].as_u64().or_else(|| details["context_length"].as_u64()),
        configured_context: show["parameters"].as_str().and_then(configured_context),
        capabilities,
        geometry: Geometry::from_model_info(info),
        tag,
    }
}

/// Where the Ollama program would be. `OVERSEER_OLLAMA_CANDIDATES` (colon-separated) replaces the
/// list, so a test can describe a machine without it.
pub fn ollama_program() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = match std::env::var("OVERSEER_OLLAMA_CANDIDATES") {
        Ok(list) => list.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect(),
        Err(_) => {
            let mut c: Vec<PathBuf> = vec!["/Applications/Ollama.app/Contents/Resources/ollama".into(), "/usr/local/bin/ollama".into(), "/opt/homebrew/bin/ollama".into(), "/usr/bin/ollama".into()];
            c.extend(std::env::var("PATH").unwrap_or_default().split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("ollama")));
            c
        }
    };
    // Overseer's own copy (installed when the owner allowed it) comes last: the user's is preferred.
    candidates.push(crate::ollama_install::own_program());
    candidates.into_iter().find(|p| p.is_file())
}

pub fn models_dir() -> PathBuf {
    std::env::var_os("OLLAMA_MODELS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".ollama/models"))
}

pub fn ollama_status() -> Ollama {
    let url = ollama_url().unwrap_or_else(|e| format!("refused: {e}"));
    let program = ollama_program();
    let running = get("/api/version", 3);
    let version = running.as_ref().ok().and_then(|v| text(&v["version"]));
    let detail = match (&program, &running) {
        (_, Ok(_)) => "Ollama is running".to_string(),
        (Some(_), Err(e)) => format!("Ollama is installed but not running ({e})"),
        (None, Err(_)) => "Ollama is not installed".to_string(),
    };
    Ollama { installed: program.map(|p| p.display().to_string()), version, running: running.is_ok(), url, detail }
}

pub fn loaded() -> Result<Vec<Loaded>> {
    let ps = get("/api/ps", 5)?;
    Ok(ps["models"].as_array().map(|a| a.iter().map(|m| Loaded { tag: m["name"].as_str().unwrap_or_default().to_string(), size: m["size"].as_u64().unwrap_or(0), size_vram: m["size_vram"].as_u64(), context: m["context_length"].as_u64(), expires_at: text(&m["expires_at"]) }).collect()).unwrap_or_default())
}

pub fn installed_models() -> Result<Vec<Model>> {
    let tags = get("/api/tags", 10)?;
    let list = tags["models"].as_array().cloned().unwrap_or_default();
    Ok(list
        .iter()
        .map(|entry| {
            let show = post("/api/show", &json!({"model": entry["name"]}), 10).unwrap_or(Value::Null);
            model_from(entry, &show)
        })
        .collect())
}

pub fn inventory() -> Inventory {
    let (memory, memory_error) = match sys::memory() {
        Ok(m) => (Some(m), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let ollama = ollama_status();
    let (models, loaded) = if ollama.running { (installed_models().unwrap_or_default(), loaded().unwrap_or_default()) } else { (Vec::new(), Vec::new()) };
    let dir = models_dir();
    Inventory { memory, memory_error, ollama, models, loaded, disk_free: sys::disk_free(&dir), models_dir: dir.display().to_string() }
}

// ------------------------------------------------------------------ catalogue

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub tag: String,
    pub tier: u64,
    pub parameters: Option<u64>,
    pub disk_bytes: u64,
    pub max_context: u64,
    pub geometry: Option<Geometry>,
    pub verified: Value,
}

impl Entry {
    pub fn verified_with(&self, harness: &str) -> bool {
        self.verified[harness]["status"] == "passed"
    }

    /// Why it is not verified, for the list of rejected candidates.
    pub fn verification_note(&self, harness: &str) -> String {
        match self.verified[harness]["status"].as_str() {
            Some("failed") => format!("failed its check with {harness}: {}", self.verified[harness]["note"].as_str().unwrap_or("no note")),
            _ => format!("not verified with {harness}"),
        }
    }
}

fn entries(v: &Value) -> Vec<Entry> {
    v["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| Some(Entry { tag: m["tag"].as_str()?.to_string(), tier: m["tier"].as_u64().unwrap_or(9), parameters: m["parameters"].as_u64(), disk_bytes: m["disk_bytes"].as_u64()?, max_context: m["max_context"].as_u64().unwrap_or(COMFORTABLE_CONTEXT), geometry: Geometry::from_catalogue(&m["geometry"]), verified: m["verified"].clone() }))
                .collect()
        })
        .unwrap_or_default()
}

/// The shipped catalogue, with the checks recorded on this machine laid over it
/// (`local_catalogue.json` in the data directory: the same shape; its `verified` entries win).
pub fn catalogue() -> Vec<Entry> {
    let mut list = entries(&serde_json::from_str(CATALOGUE).unwrap_or(Value::Null));
    let local = crate::paths::data_dir().join("local_catalogue.json");
    if let Some(over) = std::fs::read_to_string(local).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        for o in entries(&over) {
            match list.iter_mut().find(|e| e.tag == o.tag) {
                Some(e) => {
                    if let (Some(into), Some(from)) = (e.verified.as_object_mut(), o.verified.as_object()) {
                        into.extend(from.clone());
                    }
                }
                None => list.push(o),
            }
        }
    }
    list
}

// ------------------------------------------------------------------ budget and fit

#[derive(Clone, Debug)]
pub struct PickOptions {
    /// Share of total memory one model may take, in percent. The daemon refuses more than 50.
    pub ceiling_percent: u64,
    /// Free memory that must remain after loading; `None` is max(4 GiB, 10% of total).
    pub headroom: Option<u64>,
    pub context_target: u64,
    pub context_floor: u64,
    /// Tags tried before the catalogue, in order.
    pub preferred: Vec<String>,
    pub allow_unverified: bool,
    /// The local harness the verified mark is read for.
    pub harness: String,
    /// Downloads are allowed and the registry can be reached.
    pub may_download: bool,
}

impl Default for PickOptions {
    fn default() -> Self {
        Self { ceiling_percent: 40, headroom: None, context_target: 65536, context_floor: 16384, preferred: Vec::new(), allow_unverified: false, harness: "opencode".into(), may_download: false }
    }
}

pub const MAX_CEILING_PERCENT: u64 = 50;

#[derive(Serialize, Clone, Debug)]
pub struct Budget {
    pub total: u64,
    pub available: u64,
    pub headroom: u64,
    pub ceiling_percent: u64,
    /// total × ceiling: protects the machine from the model.
    pub ceiling_share: u64,
    /// available − headroom: protects the model from everything else that is running.
    pub ceiling_now: u64,
    /// The system's own account of free memory, where it gives one (macOS), less the floor kept
    /// so that a load does not take the machine to where it reports pressure.
    pub ceiling_level: Option<u64>,
    pub level: Option<i64>,
    pub level_floor: i64,
    pub budget: u64,
    pub pressure: Pressure,
    pub note: Option<String>,
}

/// The system's own free-memory percentage that a load must leave. On the machine this was
/// built on (macOS, 128 GiB) the system reported pressure from about 40% down, while "available"
/// still read 24 GiB: a 14 GiB load that fitted `available − headroom` took it there (AC-87's
/// evaluation of 2026-09-26). Five points are kept above that.
pub const LEVEL_FLOOR_PERCENT: i64 = 45;

pub fn headroom(total: u64, configured: Option<u64>) -> u64 {
    configured.unwrap_or_else(|| (4 * GIB).max(total / 10))
}

/// `min(total × ceiling, available − headroom)`, and where the system gives its own free-memory
/// level, no more than what keeps that level above the floor. `reclaimable` is memory the
/// candidate itself already holds (it is loaded), which loading it again would not take a second
/// time. A warning from the system lowers the ceiling by ten points; at critical pressure nothing
/// may be loaded.
pub fn budget(mem: &Memory, opts: &PickOptions, reclaimable: u64) -> Budget {
    let mut percent = opts.ceiling_percent.min(MAX_CEILING_PERCENT);
    let mut note = None;
    if mem.pressure == Pressure::Warn {
        percent = percent.saturating_sub(10).max(10);
        note = Some("the system reports memory pressure; the ceiling is ten points lower".to_string());
    }
    let headroom = headroom(mem.total, opts.headroom);
    let ceiling_share = mem.total / 100 * percent;
    let ceiling_now = (mem.available + reclaimable).saturating_sub(headroom);
    let ceiling_level = mem.level.map(|l| mem.total / 100 * (l - LEVEL_FLOOR_PERCENT).clamp(0, 100) as u64 + reclaimable);
    let mut budget = ceiling_share.min(ceiling_now).min(ceiling_level.unwrap_or(u64::MAX));
    if mem.pressure == Pressure::Critical {
        budget = 0;
        note = Some("the system reports critical memory pressure; nothing may be loaded".to_string());
    }
    Budget { total: mem.total, available: mem.available, headroom, ceiling_percent: percent, ceiling_share, ceiling_now, ceiling_level, level: mem.level, level_floor: LEVEL_FLOOR_PERCENT, budget, pressure: mem.pressure, note }
}

/// `weights + KV cache(context) + overhead`. `None` when the geometry is unknown: a size that
/// cannot be estimated is never assumed.
pub fn estimate(weights: u64, geometry: Option<&Geometry>, context: u64) -> Option<u64> {
    Some(weights + geometry?.kv_bytes_per_token() * context + OVERHEAD)
}

/// Measured sizes by (base tag, context), from Ollama's own numbers.
pub type Measured = HashMap<(String, u64), u64>;

#[derive(Serialize, Clone, Debug)]
pub struct Choice {
    /// The catalogue's tag (the base model).
    pub tag: String,
    /// The tag to run: an installed one that already sets this context, or the derived tag
    /// Overseer creates (`overseer/<model>:<n>k`, which shares the weights and takes no disk).
    pub run_tag: String,
    pub run_tag_exists: bool,
    pub context: u64,
    pub bytes: u64,
    /// True when `bytes` is Ollama's own measurement, false when it is the estimate.
    pub measured: bool,
    pub tier: u64,
    pub installed: bool,
    /// Bytes to download first, when the model is not installed.
    pub download_bytes: Option<u64>,
    pub verified: bool,
    pub budget: Budget,
}

#[derive(Serialize, Clone, Debug)]
pub struct Rejected {
    pub tag: String,
    pub reason: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Pick {
    pub chosen: Option<Choice>,
    /// Every other candidate that fits, in the order it would be taken.
    pub alternatives: Vec<Choice>,
    pub rejected: Vec<Rejected>,
    pub budget: Budget,
}

/// The name of Overseer's own tag for `base` at `context`. The context is the tag itself, so
/// that Ollama keeps the name as it is (a name without a tag becomes `<name>:latest`).
pub fn derived_tag(base: &str, context: u64) -> String {
    format!("overseer/{}:{}k", base.replace([':', '/'], "-"), context / 1024)
}

/// A model's name as Ollama lists it: a name without a tag means `latest`.
pub fn canonical(tag: &str) -> String {
    match tag.rsplit('/').next() {
        Some(last) if !last.contains(':') => format!("{tag}:latest"),
        _ => tag.to_string(),
    }
}

/// The contexts tried for a model: the target (or the model's own maximum when that is smaller),
/// then half of it, down to the floor.
pub fn context_steps(target: u64, floor: u64, max: u64) -> Vec<u64> {
    let mut steps = Vec::new();
    let mut c = target.min(max);
    while c >= floor && c > 0 {
        steps.push(c);
        c /= 2;
    }
    steps
}

struct Candidate {
    tag: String,
    tier: u64,
    /// Smaller comes first within a tier.
    order: u64,
    weights: u64,
    max_context: u64,
    geometry: Option<Geometry>,
    installed: Vec<Model>,
    verified: bool,
    note: String,
    tools: bool,
}

/// The best local model that fits, and why every other candidate was not taken. Pure: everything
/// it decides from is passed in, so it can be tested for any machine.
pub fn pick(mem: &Memory, models: &[Model], loaded: &[Loaded], catalogue: &[Entry], measured: &Measured, opts: &PickOptions) -> Pick {
    let base_budget = budget(mem, opts, 0);
    let mut candidates: Vec<Candidate> = Vec::new();
    let installed_as = |tag: &str| -> Vec<Model> { models.iter().filter(|m| m.base == tag || m.tag == tag).cloned().collect() };
    // `order` keeps the owner's own order within tier 0; elsewhere the larger model comes first.
    let add = |candidates: &mut Vec<Candidate>, tag: &str, tier: u64, entry: Option<&Entry>, order: Option<u64>| {
        if candidates.iter().any(|c| c.tag == tag) {
            return;
        }
        let installed = installed_as(tag);
        let first = installed.iter().find(|m| m.tag == tag).or(installed.first());
        if entry.is_none() && first.is_none() {
            return;
        }
        candidates.push(Candidate {
            tag: tag.to_string(),
            tier,
            order: order.unwrap_or_else(|| u64::MAX - first.and_then(|m| m.parameters).or(entry.and_then(|e| e.parameters)).unwrap_or(0)),
            weights: first.map(|m| m.size).filter(|s| *s > 0).or(entry.map(|e| e.disk_bytes)).unwrap_or(0),
            max_context: first.and_then(|m| m.max_context).or(entry.map(|e| e.max_context)).unwrap_or(COMFORTABLE_CONTEXT),
            geometry: first.and_then(|m| m.geometry.clone()).or(entry.and_then(|e| e.geometry.clone())),
            tools: first.map(|m| m.capabilities.iter().any(|c| c == "tools")).unwrap_or(true),
            verified: entry.is_some_and(|e| e.verified_with(&opts.harness)),
            note: entry.map(|e| e.verification_note(&opts.harness)).unwrap_or_else(|| format!("not in the catalogue, so not verified with {}", opts.harness)),
            installed,
        });
    };
    for (i, tag) in opts.preferred.iter().enumerate() {
        add(&mut candidates, tag, 0, catalogue.iter().find(|e| &e.tag == tag), Some(i as u64));
    }
    for e in catalogue {
        add(&mut candidates, &e.tag, e.tier, Some(e), None);
    }
    if opts.allow_unverified {
        for m in models {
            add(&mut candidates, &m.base, 5, None, None);
        }
    }

    let mut fits: Vec<(u64, Choice)> = Vec::new();
    let mut rejected: Vec<Rejected> = Vec::new();
    for c in candidates {
        let reject = |reason: String, rejected: &mut Vec<Rejected>| rejected.push(Rejected { tag: c.tag.clone(), reason });
        if !c.tools {
            reject("Ollama does not report tool calling for it".into(), &mut rejected);
            continue;
        }
        if !c.verified && !opts.allow_unverified {
            reject(c.note.clone(), &mut rejected);
            continue;
        }
        let is_installed = !c.installed.is_empty();
        if !is_installed && !opts.may_download {
            reject("not installed, and downloads are off or the registry cannot be reached".into(), &mut rejected);
            continue;
        }
        if c.weights == 0 {
            reject("its size is unknown".into(), &mut rejected);
            continue;
        }
        let steps = context_steps(opts.context_target, opts.context_floor, c.max_context);
        if steps.is_empty() {
            reject(format!("its longest context ({}) is below the floor ({})", c.max_context, opts.context_floor), &mut rejected);
            continue;
        }
        let mut chosen = None;
        let mut smallest: Option<(u64, u64, u64)> = None;
        for ctx in steps {
            // A copy that is already loaded at this context or more costs nothing to use again.
            let held = loaded.iter().filter(|l| c.installed.iter().any(|m| m.tag == l.tag) || l.tag == c.tag).filter(|l| l.context.unwrap_or(0) >= ctx).map(|l| l.size).max().unwrap_or(0);
            let b = budget(mem, opts, held);
            let known = measured.get(&(c.tag.clone(), ctx)).copied();
            let Some(bytes) = known.or_else(|| estimate(c.weights, c.geometry.as_ref(), ctx)) else {
                break;
            };
            smallest = Some((bytes, ctx, b.budget));
            if bytes <= b.budget {
                let existing = c.installed.iter().find(|m| m.configured_context == Some(ctx));
                chosen = Some(Choice {
                    tag: c.tag.clone(),
                    run_tag: existing.map(|m| m.tag.clone()).unwrap_or_else(|| derived_tag(&c.tag, ctx)),
                    run_tag_exists: existing.is_some(),
                    context: ctx,
                    bytes,
                    measured: known.is_some(),
                    tier: c.tier,
                    installed: is_installed,
                    download_bytes: (!is_installed).then_some(c.weights),
                    verified: c.verified,
                    budget: b,
                });
                break;
            }
        }
        match (chosen, smallest) {
            (Some(choice), _) => fits.push((c.order, choice)),
            (None, Some((bytes, ctx, b))) => reject(format!("too big: {} GiB at a {}k context is over the budget of {} GiB", gib(bytes), ctx / 1024, gib(b)), &mut rejected),
            (None, None) => reject("its size cannot be estimated (no measurement and no geometry)".into(), &mut rejected),
        }
    }
    // Models that only fit below the comfortable context come after every model that fits at it.
    // Then the tier (the owner's own list first, in the owner's order); within a tier what is
    // installed before what needs a download, the longer context, and the larger model.
    fits.sort_by_key(|(order, f)| (f.context < COMFORTABLE_CONTEXT, f.tier, if f.tier == 0 { *order } else { 0 }, !f.installed, std::cmp::Reverse(f.context), *order));
    let mut it = fits.into_iter().map(|(_, f)| f);
    Pick { chosen: it.next(), alternatives: it.collect(), rejected, budget: base_budget }
}

// ------------------------------------------------------------------ measured sizes

pub fn ensure_tables(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_models_measured(tag TEXT NOT NULL, ctx INTEGER NOT NULL, bytes INTEGER NOT NULL, ollama_version TEXT, at INTEGER NOT NULL, PRIMARY KEY(tag, ctx));
         CREATE TABLE IF NOT EXISTS local_loads(tag TEXT PRIMARY KEY, at INTEGER NOT NULL);",
    )?;
    Ok(())
}

/// The tags Overseer loaded itself. Only these are ever unloaded to make room: a model the user
/// loaded in their own Ollama is left alone.
pub fn loaded_by_overseer(conn: &rusqlite::Connection) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    if let Ok(mut stmt) = conn.prepare("SELECT tag FROM local_loads") {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            out.extend(rows.flatten());
        }
    }
    out
}

pub fn note_loaded(conn: &rusqlite::Connection, tag: &str, at: i64) {
    let _ = conn.execute("INSERT INTO local_loads(tag, at) VALUES(?1, ?2) ON CONFLICT(tag) DO UPDATE SET at=excluded.at", rusqlite::params![tag, at]);
}

pub fn note_unloaded(conn: &rusqlite::Connection, tag: &str) {
    let _ = conn.execute("DELETE FROM local_loads WHERE tag=?1", [tag]);
}

pub fn measured(conn: &rusqlite::Connection) -> Measured {
    let mut out = Measured::new();
    if let Ok(mut stmt) = conn.prepare("SELECT tag, ctx, bytes FROM local_models_measured") {
        if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))) {
            for (tag, ctx, bytes) in rows.flatten() {
                out.insert((tag, ctx as u64), bytes as u64);
            }
        }
    }
    out
}

/// Records what Ollama reports for every loaded model, under its base tag. Returns what was new.
pub fn record_measured(conn: &rusqlite::Connection, models: &[Model], loaded: &[Loaded], version: Option<&str>, now: i64) -> Result<Vec<(String, u64, u64)>> {
    let mut new = Vec::new();
    for l in loaded {
        let (Some(ctx), true) = (l.context, l.size > 0) else { continue };
        let base = models.iter().find(|m| m.tag == l.tag).map(|m| m.base.clone()).unwrap_or_else(|| l.tag.clone());
        let before: Option<i64> = conn.query_row("SELECT bytes FROM local_models_measured WHERE tag=?1 AND ctx=?2", rusqlite::params![base, ctx as i64], |r| r.get(0)).ok();
        if before != Some(l.size as i64) {
            conn.execute("INSERT INTO local_models_measured(tag, ctx, bytes, ollama_version, at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tag, ctx) DO UPDATE SET bytes=excluded.bytes, ollama_version=excluded.ollama_version, at=excluded.at", rusqlite::params![base, ctx as i64, l.size as i64, version, now])?;
            new.push((base, ctx, l.size));
        }
    }
    Ok(new)
}

// ------------------------------------------------------------------ the guard (AC-140)

/// May this model be loaded at this context right now? Decided from a fresh memory reading and
/// Ollama's own list of loaded models, never from an earlier pick. Every path that loads a model
/// asks here first, and there is no override.
pub fn approve(tag: &str, context: u64, models: &[Model], loaded: &[Loaded], catalogue: &[Entry], measured: &Measured, opts: &PickOptions) -> Result<Value> {
    let mem = sys::memory().context("memory cannot be read, so nothing is loaded")?;
    approve_with(&mem, tag, context, models, loaded, catalogue, measured, opts)
}

#[allow(clippy::too_many_arguments)]
pub fn approve_with(mem: &Memory, tag: &str, context: u64, models: &[Model], loaded: &[Loaded], catalogue: &[Entry], measured: &Measured, opts: &PickOptions) -> Result<Value> {
    let model = models.iter().find(|m| m.tag == tag);
    let base = model.map(|m| m.base.clone()).unwrap_or_else(|| tag.to_string());
    let entry = catalogue.iter().find(|e| e.tag == base);
    let weights = model.map(|m| m.size).filter(|s| *s > 0).or(entry.map(|e| e.disk_bytes)).ok_or_else(|| anyhow!("{tag} is not installed and not in the catalogue; its size is unknown, so it is not loaded"))?;
    let geometry = model.and_then(|m| m.geometry.clone()).or_else(|| entry.and_then(|e| e.geometry.clone()));
    let known = measured.get(&(base.clone(), context)).copied();
    let bytes = known.or_else(|| estimate(weights, geometry.as_ref(), context)).ok_or_else(|| anyhow!("the size of {tag} cannot be estimated, so it is not loaded"))?;
    // A copy of this model that is already loaded at this context or more, under this tag or any
    // other tag of the same base, costs nothing to use again.
    let same_base = |l: &Loaded| l.tag == tag || l.tag == base || models.iter().any(|m| m.tag == l.tag && m.base == base);
    let held = loaded.iter().filter(|l| same_base(l) && l.context.unwrap_or(0) >= context).map(|l| l.size).max().unwrap_or(0);
    let b = budget(mem, opts, held);
    // Another model that is loaded stays loaded: the two together must fit the share.
    let beside: Vec<&Loaded> = loaded.iter().filter(|l| !same_base(l)).collect();
    let others: u64 = beside.iter().map(|l| l.size).sum();
    if held == 0 && others > 0 && bytes + others > b.ceiling_share {
        bail!("{tag} does not fit beside {}: {} GiB and {} GiB together are over {}% of {} GiB ({} GiB)", beside.iter().map(|l| l.tag.as_str()).collect::<Vec<_>>().join(" and "), gib(bytes), gib(others), b.ceiling_percent, gib(b.total), gib(b.ceiling_share));
    }
    if bytes > b.budget {
        bail!("{tag} is too big to load: {} GiB at a {}k context is over the budget of {} GiB ({}% of {} GiB is {} GiB; {} GiB available minus {} GiB headroom is {} GiB{}){}", gib(bytes), context / 1024, gib(b.budget), b.ceiling_percent, gib(b.total), gib(b.ceiling_share), gib(b.available + held), gib(b.headroom), gib(b.ceiling_now), match (b.level, b.ceiling_level) {
            (Some(level), Some(left)) => format!("; the system counts {level}% of memory as free and {}% is kept free, which leaves {} GiB", b.level_floor, gib(left)),
            _ => String::new(),
        }, b.note.as_ref().map(|n| format!("; {n}")).unwrap_or_default());
    }
    Ok(json!({"tag": tag, "base": base, "context": context, "bytes": bytes, "measured": known.is_some(), "already_loaded": held > 0, "budget": b}))
}

/// The tag to run `base` at `context`: `tag` itself or another installed tag when it already sets
/// that context, otherwise Overseer's own derived tag, created from the base (it shares the
/// weights, so it takes no disk and is made at once).
pub fn ensure_tag(models: &[Model], tag: &str, base: &str, context: u64) -> Result<String> {
    if let Some(m) = models.iter().find(|m| m.tag == tag && m.configured_context == Some(context)) {
        return Ok(m.tag.clone());
    }
    if let Some(m) = models.iter().find(|m| m.base == base && m.configured_context == Some(context)) {
        return Ok(m.tag.clone());
    }
    if !models.iter().any(|m| m.tag == base || m.tag == tag) {
        bail!("{base} is not installed");
    }
    let from = if models.iter().any(|m| m.tag == base) { base } else { tag };
    let name = derived_tag(base, context);
    let answer = post("/api/create", &json!({"model": name, "from": from, "parameters": {"num_ctx": context}, "stream": false}), 120)?;
    if answer["status"] != "success" {
        bail!("Ollama did not create {name}: {answer}");
    }
    Ok(name)
}

/// Unloads a model (`keep_alive: 0`).
pub fn unload(tag: &str) -> Result<()> {
    post("/api/generate", &json!({"model": tag, "keep_alive": 0}), 30).map(|_| ())
}

/// Should a load in progress be stopped? Available memory under half the headroom, or the
/// system's critical pressure.
pub fn must_stop_load(mem: &Memory, headroom: u64) -> Option<String> {
    if mem.pressure == Pressure::Critical {
        return Some("the system reports critical memory pressure".into());
    }
    (mem.available < headroom / 2).then(|| format!("available memory fell to {} GiB, under half the headroom of {} GiB", gib(mem.available), gib(headroom)))
}

/// Loads an approved model while watching memory once a second. If memory runs short the load is
/// cancelled, the model unloaded, and the reason returned as the error.
pub fn load_guarded(tag: &str, context: u64, headroom: u64, keep_alive: &str) -> Result<Value> {
    let (tx, rx) = std::sync::mpsc::channel();
    let body = json!({"model": tag, "prompt": "", "keep_alive": keep_alive, "options": {"num_ctx": context}});
    std::thread::spawn(move || {
        let _ = tx.send(post("/api/generate", &body, 600));
    });
    let started = std::time::Instant::now();
    let mut samples = 0u64;
    let mut lowest = u64::MAX;
    loop {
        match rx.recv_timeout(watch_interval()) {
            Ok(result) => {
                result?;
                return Ok(json!({"tag": tag, "context": context, "load_ms": started.elapsed().as_millis() as u64, "samples": samples, "lowest_available": if lowest == u64::MAX { Value::Null } else { json!(lowest) }}));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let mem = sys::memory()?;
                samples += 1;
                lowest = lowest.min(mem.available);
                if let Some(why) = must_stop_load(&mem, headroom) {
                    let _ = unload(tag);
                    bail!("the load of {tag} was cancelled and the model unloaded: {why}");
                }
            }
            Err(_) => bail!("the load of {tag} ended without an answer"),
        }
    }
}

fn watch_interval() -> Duration {
    Duration::from_millis(std::env::var("OVERSEER_TEST_WATCH_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1000))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(total_gib: u64, available_gib: f64) -> Memory {
        Memory { total: total_gib * GIB, available: (available_gib * GIB as f64) as u64, pressure: Pressure::Normal, level: None, source: "test".into() }
    }

    /// A machine with nothing else running: everything but 10% is available.
    fn idle(total_gib: u64) -> Memory {
        mem(total_gib, total_gib as f64 * 0.9)
    }

    fn all_verified() -> Vec<Entry> {
        entries(&serde_json::from_str(CATALOGUE).unwrap()).into_iter().map(|mut e| { e.verified = json!({"opencode": {"status": "passed"}}); e }).collect()
    }

    fn shipped() -> Vec<Entry> {
        entries(&serde_json::from_str(CATALOGUE).unwrap())
    }

    fn installed(tag: &str, entry: &Entry) -> Model {
        Model { tag: tag.into(), base: entry.tag.clone(), size: entry.disk_bytes, family: None, parameter_size: None, parameters: entry.parameters, quantization: None, max_context: Some(entry.max_context), configured_context: None, capabilities: vec!["completion".into(), "tools".into()], geometry: entry.geometry.clone() }
    }

    fn downloads() -> PickOptions {
        PickOptions { may_download: true, ..Default::default() }
    }

    fn line(c: &Choice) -> String {
        format!("{} at {}k", c.tag, c.context / 1024)
    }

    #[test]
    fn kv_cache_per_token_for_both_shapes() {
        let qwen3 = Geometry::from_model_info(&json!({"general.architecture": "qwen3moe", "qwen3moe.block_count": 48, "qwen3moe.attention.head_count_kv": 4, "qwen3moe.attention.key_length": 128, "qwen3moe.attention.value_length": 128})).unwrap();
        assert_eq!(qwen3.kv_bytes_per_token(), 98_304);
        // Older models give no key length: the embedding width over the head count is used.
        let qwen25 = Geometry::from_model_info(&json!({"general.architecture": "qwen2", "qwen2.block_count": 48, "qwen2.attention.head_count_kv": 8, "qwen2.attention.head_count": 40, "qwen2.embedding_length": 5120})).unwrap();
        assert_eq!(qwen25.kv_bytes_per_token(), 196_608);
        // Hybrid attention: a list per layer, zero where a layer keeps no cache.
        let mut per_layer = vec![0u64; 40];
        for i in (3..40).step_by(4) {
            per_layer[i] = 2;
        }
        let qwen35 = Geometry::from_model_info(&json!({"general.architecture": "qwen35moe", "qwen35moe.block_count": 40, "qwen35moe.attention.head_count_kv": per_layer, "qwen35moe.attention.key_length": 256, "qwen35moe.attention.value_length": 256})).unwrap();
        assert_eq!(qwen35.kv_bytes_per_token(), 10 * 2 * 512 * 2);
        assert!(Geometry::from_model_info(&json!({"general.architecture": "x"})).is_none(), "unknown geometry stays unknown");
    }

    #[test]
    fn the_estimate_is_within_15_percent_of_the_measured_size() {
        let e = shipped().into_iter().find(|e| e.tag == "qwen3-coder:30b").unwrap();
        let est = estimate(e.disk_bytes, e.geometry.as_ref(), 65536).unwrap();
        assert_eq!(gib(est), 24.3);
        let measured = 25_411_736_042u64; // Ollama's own number on 2026-09-26, 65,536-token context
        assert!((est.abs_diff(measured) as f64 / measured as f64) < 0.15);
        assert!(estimate(e.disk_bytes, None, 65536).is_none(), "no geometry, no estimate");
    }

    #[test]
    fn the_systems_own_free_level_is_a_third_bound() {
        let o = PickOptions::default();
        let at = |available: f64, level: i64| Memory { level: Some(level), ..mem(128, available) };
        // An idle machine (75% free by the system's count): 30 points above the floor are 38.4 GiB.
        let idle = budget(&at(115.2, 75), &o, 0);
        assert_eq!((gib(idle.ceiling_share), gib(idle.ceiling_now), idle.ceiling_level.map(gib), gib(idle.budget)), (51.2, 102.4, Some(38.4), 38.4));
        // The busy machine of 2026-09-26: 30.5 GiB read as available and the level was 47%. The
        // two older terms allowed 17.7 GiB, and a 14.2 GiB load took the system to its warning.
        let busy = budget(&at(30.5, 47), &o, 0);
        assert_eq!((gib(busy.ceiling_now), busy.ceiling_level.map(gib), gib(busy.budget)), (17.7, Some(2.6), 2.6));
        // At the floor and under it nothing new fits; a copy that is already loaded still does.
        assert_eq!(budget(&at(30.0, 45), &o, 0).budget, 0);
        assert_eq!(budget(&at(30.0, 12), &o, 0).budget, 0);
        assert_eq!(gib(budget(&at(10.0, 40), &o, 24 * GIB).budget), 21.2, "what it already holds is not taken twice");
        // Where the system gives no level (Linux, fixtures) the two terms decide alone.
        assert_eq!((budget(&mem(128, 30.5), &o, 0).ceiling_level, gib(budget(&mem(128, 30.5), &o, 0).budget)), (None, 17.7));
        // The refusal says all three.
        let cat = shipped();
        let e14 = cat.iter().find(|e| e.tag == "qwen2.5-coder:14b").unwrap();
        let err = approve_with(&at(30.5, 47), "qwen2.5-coder:14b", 32768, &[installed("qwen2.5-coder:14b", e14)], &[], &cat, &Measured::new(), &o).unwrap_err().to_string();
        assert_eq!(err, "qwen2.5-coder:14b is too big to load: 15.4 GiB at a 32k context is over the budget of 2.6 GiB (40% of 128 GiB is 51.2 GiB; 30.5 GiB available minus 12.8 GiB headroom is 17.7 GiB; the system counts 47% of memory as free and 45% is kept free, which leaves 2.6 GiB)");
    }

    #[test]
    fn the_budget_has_two_terms_and_a_hard_ceiling() {
        let o = PickOptions::default();
        let b = budget(&idle(128), &o, 0);
        assert_eq!((gib(b.ceiling_share), gib(b.headroom)), (51.2, 12.8));
        assert_eq!(gib(b.budget), 51.2, "the share decides on an idle machine");
        let busy = budget(&mem(128, 28.0), &o, 0);
        assert_eq!(gib(busy.budget), 15.2, "with 100 GiB in use, what is free now decides");
        assert_eq!(gib(budget(&idle(8), &o, 0).headroom), 4.0, "headroom is never under 4 GiB");
        let greedy = budget(&idle(128), &PickOptions { ceiling_percent: 80, ..Default::default() }, 0);
        assert_eq!(greedy.ceiling_percent, 50, "the ceiling never goes over 50%");
        let warn = budget(&Memory { pressure: Pressure::Warn, ..idle(128) }, &o, 0);
        assert_eq!(warn.ceiling_percent, 30);
        let critical = budget(&Memory { pressure: Pressure::Critical, ..idle(128) }, &o, 0);
        assert_eq!(critical.budget, 0);
        assert_eq!(gib(budget(&mem(128, 20.0), &o, 24 * GIB).ceiling_now), 31.2, "a copy that is already loaded is not counted twice");
    }

    #[test]
    fn contexts_halve_from_the_target_to_the_floor() {
        assert_eq!(context_steps(65536, 16384, 262144), vec![65536, 32768, 16384]);
        assert_eq!(context_steps(65536, 16384, 32768), vec![32768, 16384]);
        assert_eq!(context_steps(131072, 16384, 262144), vec![131072, 65536, 32768, 16384]);
        assert!(context_steps(65536, 16384, 8192).is_empty());
    }

    /// The RFC's worked examples: machine profiles with memory otherwise free and a 40% ceiling.
    #[test]
    fn the_worked_examples_for_16_32_64_and_128_gib() {
        let cat = all_verified();
        let picks = |total: u64| -> Vec<String> {
            let p = pick(&idle(total), &[], &[], &cat, &Measured::new(), &downloads());
            p.chosen.iter().chain(p.alternatives.iter()).map(line).collect()
        };
        let p16 = picks(16);
        assert_eq!(&p16[..3], ["qwen2.5-coder:3b at 32k", "qwen2.5-coder:1.5b at 32k", "qwen2.5-coder:7b at 16k"], "16 GiB, budget 6.4 GiB");
        let p32 = picks(32);
        assert_eq!(&p32[..2], ["qwen2.5-coder:7b at 32k", "qwen2.5-coder:3b at 32k"], "32 GiB, budget 12.8 GiB");
        assert!(p32.contains(&"qwen2.5-coder:14b at 16k".to_string()), "the 14B fits only at 16k, so it comes after those at 32k");
        let p64 = picks(64);
        assert_eq!(p64[0], "qwen3-coder:30b at 64k", "64 GiB, budget 25.6 GiB");
        assert!(!p64.iter().any(|l| l.starts_with("qwen2.5-coder:32b at 32k")), "27.6 GiB does not fit 25.6 GiB");
        let p128 = picks(128);
        assert_eq!(&p128[..2], ["qwen3-coder:30b at 64k", "qwen2.5-coder:32b at 32k"], "128 GiB, budget 51.2 GiB");
        // Never above the budget, on any profile.
        for total in [8, 16, 24, 32, 48, 64, 96, 128] {
            let p = pick(&idle(total), &[], &[], &cat, &Measured::new(), &downloads());
            for c in p.chosen.iter().chain(p.alternatives.iter()) {
                assert!(c.bytes <= c.budget.budget, "{} on {total} GiB", line(c));
            }
        }
    }

    #[test]
    fn a_raised_target_gives_the_longer_context_when_it_fits() {
        let p = pick(&idle(128), &[], &[], &all_verified(), &Measured::new(), &PickOptions { context_target: 131072, ..downloads() });
        let c = p.chosen.unwrap();
        assert_eq!((line(&c).as_str(), gib(c.bytes)), ("qwen3-coder:30b at 128k", 30.3));
    }

    #[test]
    fn with_100_gib_in_use_the_pick_drops_to_the_14b_at_16k() {
        let cat = all_verified();
        let models: Vec<Model> = cat.iter().filter(|e| ["qwen3-coder:30b", "qwen2.5-coder:14b"].contains(&e.tag.as_str())).map(|e| installed(&e.tag, e)).collect();
        let p = pick(&mem(128, 28.0), &models, &[], &cat, &Measured::new(), &PickOptions::default());
        assert_eq!(line(p.chosen.as_ref().unwrap()), "qwen2.5-coder:14b at 16k");
        let big = p.rejected.iter().find(|r| r.tag == "qwen3-coder:30b").unwrap();
        assert!(big.reason.starts_with("too big: 19.8 GiB at a 16k context is over the budget of 15.2 GiB"), "{}", big.reason);
        assert!(p.rejected.iter().any(|r| r.tag == "qwen2.5-coder:7b" && r.reason.starts_with("not installed")));
    }

    #[test]
    fn only_verified_models_with_tools_are_picked_on_their_own() {
        let cat = shipped();
        let mut models: Vec<Model> = cat.iter().map(|e| installed(&e.tag, e)).collect();
        let p = pick(&idle(32), &models, &[], &cat, &Measured::new(), &PickOptions::default());
        assert!(p.chosen.is_none(), "on 32 GiB nothing verified fits: {:?}", p.chosen.map(|c| line(&c)));
        // Every size of qwen2.5-coder failed its check on 2026-09-26 (AC-87), each with its reason.
        assert!(p.rejected.iter().any(|r| r.tag == "qwen2.5-coder:14b" && r.reason.starts_with("failed its check with opencode: wrote its tool calls as text")));
        assert!(p.rejected.iter().any(|r| r.tag == "qwen2.5-coder:7b" && r.reason == "failed its check with opencode: said done without calling a tool; nothing was written"));
        assert_eq!(p.rejected.iter().filter(|r| r.reason.starts_with("failed its check with opencode")).count(), 5);
        assert_eq!(cat.iter().filter(|e| e.verified_with("opencode")).map(|e| e.tag.as_str()).collect::<Vec<_>>(), ["qwen3-coder:30b"], "the one model that passed");
        assert!(p.rejected.iter().any(|r| r.tag == "qwen3-coder:30b" && r.reason.starts_with("too big")));
        let p = pick(&idle(128), &models, &[], &cat, &Measured::new(), &PickOptions::default());
        assert_eq!(line(&p.chosen.unwrap()), "qwen3-coder:30b at 64k");
        assert!(p.alternatives.is_empty());
        // The owner may allow unverified models; tool calling is still required.
        models.iter_mut().find(|m| m.tag == "qwen2.5-coder:7b").unwrap().capabilities = vec!["completion".into()];
        let p = pick(&idle(32), &models, &[], &cat, &Measured::new(), &PickOptions { allow_unverified: true, ..Default::default() });
        assert_eq!(line(&p.chosen.unwrap()), "qwen2.5-coder:3b at 32k");
        assert!(p.rejected.iter().any(|r| r.tag == "qwen2.5-coder:7b" && r.reason.contains("tool calling")));
    }

    #[test]
    fn a_measured_size_replaces_the_estimate_and_an_installed_tag_is_reused() {
        let cat = shipped();
        let e = cat.iter().find(|e| e.tag == "qwen3-coder:30b").unwrap();
        let mut derived = installed("qwen3-coder:30b-64k", e);
        derived.configured_context = Some(65536);
        let models = vec![installed("qwen3-coder:30b", e), derived];
        let mut m = Measured::new();
        m.insert(("qwen3-coder:30b".into(), 65536), 25_411_736_042);
        let c = pick(&idle(128), &models, &[], &cat, &m, &PickOptions::default()).chosen.unwrap();
        assert!(c.measured);
        assert_eq!(gib(c.bytes), 23.7);
        assert_eq!((c.run_tag.as_str(), c.run_tag_exists), ("qwen3-coder:30b-64k", true));
        let c = pick(&idle(128), &models[..1], &[], &cat, &Measured::new(), &PickOptions::default()).chosen.unwrap();
        assert_eq!((c.run_tag.as_str(), c.run_tag_exists, c.measured), ("overseer/qwen3-coder-30b:64k", false, false));
    }

    #[test]
    fn the_owners_order_comes_first() {
        let cat = all_verified();
        let p = pick(&idle(128), &[], &[], &cat, &Measured::new(), &PickOptions { preferred: vec!["qwen2.5-coder:7b".into(), "qwen2.5-coder:32b".into()], ..downloads() });
        let order: Vec<String> = p.chosen.iter().chain(p.alternatives.iter()).map(line).collect();
        assert_eq!(&order[..3], ["qwen2.5-coder:7b at 32k", "qwen2.5-coder:32b at 32k", "qwen3-coder:30b at 64k"]);
    }

    #[test]
    fn the_guard_refuses_anything_over_the_budget() {
        let cat = shipped();
        let e30 = cat.iter().find(|e| e.tag == "qwen3-coder:30b").unwrap();
        let huge = Model { tag: "qwen3.5:122b".into(), base: "qwen3.5:122b".into(), size: 81_400_000_000, family: None, parameter_size: Some("125.1B".into()), parameters: Some(125_086_497_008), quantization: None, max_context: Some(262144), configured_context: None, capabilities: vec!["tools".into()], geometry: Some(Geometry { kv_heads_per_layer: vec![2; 12], key_length: 256, value_length: 256 }) };
        let models = vec![installed("qwen3-coder:30b", e30), huge];
        let o = PickOptions::default();
        let err = approve_with(&idle(128), "qwen3.5:122b", 16384, &models, &[], &cat, &Measured::new(), &o).unwrap_err().to_string();
        assert!(err.starts_with("qwen3.5:122b is too big to load: 77.2 GiB at a 16k context is over the budget of 51.2 GiB"), "{err}");
        // Not even at the 50% maximum, and not when the owner allows unverified models.
        let widest = PickOptions { ceiling_percent: 50, allow_unverified: true, ..Default::default() };
        assert!(approve_with(&idle(128), "qwen3.5:122b", 16384, &models, &[], &cat, &Measured::new(), &widest).is_err());
        let ok = approve_with(&idle(128), "qwen3-coder:30b", 65536, &models, &[], &cat, &Measured::new(), &o).unwrap();
        assert_eq!(ok["measured"], false);
        // The same model is refused when memory is short now, and allowed again when it is the
        // copy that is already loaded.
        assert!(approve_with(&mem(128, 30.0), "qwen3-coder:30b", 65536, &models, &[], &cat, &Measured::new(), &o).is_err());
        let held = [Loaded { tag: "qwen3-coder:30b".into(), size: 25_411_736_042, size_vram: None, context: Some(65536), expires_at: None }];
        assert_eq!(approve_with(&mem(128, 30.0), "qwen3-coder:30b", 65536, &models, &held, &cat, &Measured::new(), &o).unwrap()["already_loaded"], true);
        // The loaded copy may carry another tag of the same base (a tag that sets the context).
        let mut tagged = installed("qwen3-coder:30b-64k", e30);
        tagged.configured_context = Some(65536);
        let both = vec![installed("qwen3-coder:30b", e30), tagged];
        let held = [Loaded { tag: "qwen3-coder:30b-64k".into(), size: 25_411_736_042, size_vram: None, context: Some(65536), expires_at: None }];
        assert_eq!(approve_with(&mem(128, 30.0), "qwen3-coder:30b", 65536, &both, &held, &cat, &Measured::new(), &o).unwrap()["already_loaded"], true);
        // A copy loaded with a shorter context does not count: the longer one needs more memory.
        let short = [Loaded { tag: "qwen3-coder:30b-64k".into(), size: 20_000_000_000, size_vram: None, context: Some(16384), expires_at: None }];
        assert!(approve_with(&mem(128, 30.0), "qwen3-coder:30b", 65536, &both, &short, &cat, &Measured::new(), &o).is_err());
        // Unknown size: refused, never assumed small.
        let err = approve_with(&idle(128), "mystery:latest", 16384, &models, &[], &cat, &Measured::new(), &o).unwrap_err().to_string();
        assert!(err.contains("its size is unknown"), "{err}");
        assert!(approve_with(&Memory { pressure: Pressure::Critical, ..idle(128) }, "qwen3-coder:30b", 16384, &models, &[], &cat, &Measured::new(), &o).is_err());
        // A second, different model loads only when both fit the share together.
        let e14 = cat.iter().find(|e| e.tag == "qwen2.5-coder:14b").unwrap();
        let three = vec![installed("qwen3-coder:30b", e30), installed("qwen2.5-coder:14b", e14), models[1].clone()];
        let big = [Loaded { tag: "qwen3-coder:30b".into(), size: 40 * GIB, size_vram: None, context: Some(131072), expires_at: None }];
        let err = approve_with(&mem(128, 70.0), "qwen2.5-coder:14b", 32768, &three, &big, &cat, &Measured::new(), &o).unwrap_err().to_string();
        assert_eq!(err, "qwen2.5-coder:14b does not fit beside qwen3-coder:30b: 15.4 GiB and 40 GiB together are over 40% of 128 GiB (51.2 GiB)");
        let small = [Loaded { tag: "qwen3-coder:30b".into(), size: 25 * GIB, size_vram: None, context: Some(65536), expires_at: None }];
        assert!(approve_with(&mem(128, 70.0), "qwen2.5-coder:14b", 32768, &three, &small, &cat, &Measured::new(), &o).is_ok(), "25 and 15.4 GiB fit 51.2 GiB");
    }

    #[test]
    fn a_load_is_stopped_when_memory_runs_short() {
        let headroom = headroom(128 * GIB, None);
        assert!(must_stop_load(&mem(128, 20.0), headroom).is_none());
        assert!(must_stop_load(&mem(128, 6.0), headroom).unwrap().contains("under half the headroom of 12.8 GiB"));
        assert!(must_stop_load(&Memory { pressure: Pressure::Critical, ..mem(128, 60.0) }, headroom).unwrap().contains("critical"));
    }

    #[test]
    fn ollama_is_only_addressed_on_loopback() {
        // The variable is process-wide; this is the only test that sets it.
        std::env::set_var("OVERSEER_OLLAMA_URL", "http://192.168.1.20:11434");
        assert!(ollama_url().unwrap_err().to_string().contains("loopback"));
        std::env::set_var("OVERSEER_OLLAMA_URL", "https://127.0.0.1:11434");
        assert!(ollama_url().is_err());
        std::env::set_var("OVERSEER_OLLAMA_URL", "http://[::1]:11434/");
        assert_eq!(ollama_url().unwrap(), "http://[::1]:11434");
        std::env::set_var("OVERSEER_OLLAMA_URL", "http://localhost:4000");
        assert_eq!(ollama_url().unwrap(), "http://localhost:4000");
        std::env::remove_var("OVERSEER_OLLAMA_URL");
        assert_eq!(ollama_url().unwrap(), DEFAULT_URL);
    }

    #[test]
    fn tags_and_parameters_are_read() {
        assert_eq!(configured_context("stop \"<|im_end|>\"\nnum_ctx                        65536\ntemperature 0.7"), Some(65536));
        assert_eq!(configured_context("temperature 0.7"), None);
        assert_eq!(derived_tag("qwen3-coder:30b", 65536), "overseer/qwen3-coder-30b:64k");
        assert_eq!((canonical("llama3"), canonical("qwen3-coder:30b"), canonical("overseer/x"), canonical("overseer/qwen3-coder-30b:64k")), ("llama3:latest".to_string(), "qwen3-coder:30b".to_string(), "overseer/x:latest".to_string(), "overseer/qwen3-coder-30b:64k".to_string()));
        let entry = json!({"name": "qwen3-coder:30b-64k", "size": 18556700444u64, "details": {"parent_model": "qwen3-coder:30b", "family": "qwen3moe", "parameter_size": "30.5B", "quantization_level": "Q4_K_M", "context_length": 262144}, "capabilities": ["completion", "tools"]});
        let show = json!({"parameters": "num_ctx 65536", "capabilities": ["completion", "tools"], "model_info": {"general.architecture": "qwen3moe", "general.parameter_count": 30532122624u64, "qwen3moe.block_count": 48, "qwen3moe.context_length": 262144, "qwen3moe.attention.head_count_kv": 4, "qwen3moe.attention.key_length": 128, "qwen3moe.attention.value_length": 128}});
        let m = model_from(&entry, &show);
        assert_eq!((m.base.as_str(), m.configured_context, m.max_context, m.parameters), ("qwen3-coder:30b", Some(65536), Some(262144), Some(30532122624)));
        assert_eq!(m.geometry.unwrap().kv_bytes_per_token(), 98_304);
        // With no answer from /api/show the facts from the list are kept and the rest is unknown.
        let bare = model_from(&entry, &Value::Null);
        assert_eq!((bare.geometry, bare.configured_context, bare.capabilities.len()), (None, None, 2));
    }

    #[test]
    fn measured_sizes_are_recorded_under_the_base_tag() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_tables(&conn).unwrap();
        let cat = shipped();
        let e = cat.iter().find(|e| e.tag == "qwen3-coder:30b").unwrap();
        let models = vec![installed("qwen3-coder:30b-64k", e)];
        let loaded = [Loaded { tag: "qwen3-coder:30b-64k".into(), size: 25_411_736_042, size_vram: Some(25_411_736_042), context: Some(65536), expires_at: None }];
        let new = record_measured(&conn, &models, &loaded, Some("0.34.2"), 1).unwrap();
        assert_eq!(new, vec![("qwen3-coder:30b".to_string(), 65536, 25_411_736_042)]);
        assert!(record_measured(&conn, &models, &loaded, Some("0.34.2"), 2).unwrap().is_empty(), "the same number is not recorded twice");
        assert_eq!(measured(&conn)[&("qwen3-coder:30b".to_string(), 65536)], 25_411_736_042);
    }
}
