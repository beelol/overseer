// Continuity in words (Gate L): what the status bar, the side bar, the chat cards and the
// composer say about the connection, the waiting agents and the local models. Pure functions of
// the daemon's answers, with no DOM and no VS Code, so the extension host, the webviews and the
// unit tests all use the same text. (UMD: `require` in Node, `window.OverseerContinuityText`
// in a webview.)
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.OverseerContinuityText = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  const GIB = 1024 ** 3;
  const gib = bytes => (typeof bytes !== 'number' ? '' : String(Math.round((bytes / GIB) * 10) / 10));
  const PROVIDER = { openai: 'OpenAI', anthropic: 'Claude', local: 'Local' };
  const HARNESS = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex', opencode: 'OpenCode', 'opencode-serve': 'Local model' };
  const MODE = { plan: 'Plan only', manual: 'Ask first', acceptEdits: 'Accept edits', auto: 'Auto', 'read-only': 'Read only', 'workspace-write': 'Can edit' };
  const plural = (n, one, many) => `${n} ${n === 1 ? one : many || one + 's'}`;
  // Reasons from the daemon and the harnesses in plain words (AC-245: no raw request errors or harness ids).
  const Plain = typeof require === 'function' ? require('./plain-words.js') : (typeof self !== 'undefined' ? self.OverseerPlain : undefined);
  const why = r => { if (!r || !Plain) return r || ''; const out = Plain.plain(r, 400); return out && /^[a-z]/.test(r) ? out[0].toLowerCase() + out.slice(1) : out; };

  /** Run states Continuity adds: the word, the icon, and whether the agent still holds its work. */
  const STATES = {
    waiting_for_connection: { text: 'Waiting for a connection', short: 'waiting', icon: 'cloud', active: true },
    waiting_for_memory: { text: 'Waiting for memory', short: 'waiting', icon: 'cloud', active: true },
    handed_off: { text: 'Handed off', short: 'handed off', icon: 'arrow-right', active: false },
  };
  const isWaiting = status => status === 'waiting_for_connection' || status === 'waiting_for_memory';

  /** The connection in one short label, one sentence, and the lines of its tooltip. */
  function connection(data) {
    const c = (data && data.connection) || null;
    const on = !data || !data.settings || data.settings.enabled !== false;
    if (!c) return { state: 'unknown', label: 'Connection unknown', short: '', sentence: 'Overseer has not checked the connection yet.', lines: ['Overseer has not checked the connection yet.'], icon: 'cloud' };
    const label = c.state === 'online' ? 'Online' : c.state === 'offline' ? 'Offline' : why(c.reason).replace(/^./, m => m.toUpperCase());
    const reason = c.state === 'online' ? '' : why(c.reason);
    const sentence = c.state === 'online' ? 'Overseer is online.'
      : c.state === 'offline' ? `Overseer is offline: ${reason}.`
        : `${reason.replace(/^./, m => m.toUpperCase())}. The internet works.`;
    const lines = [sentence];
    if (c.system && c.system.detail) lines.push(`The system says: ${c.system.detail}.`);
    const providers = Object.entries(c.providers || {}).map(([id, h]) => `${PROVIDER[id] || id} ${h.reachable === true ? 'reachable' : h.reachable === false ? `unreachable (${why(h.reason)})` : 'not checked'}`);
    if (providers.length && c.state !== 'offline') lines.push(providers.join(' · '));
    lines.push(policy(data));
    return { state: c.state, label, short: c.state === 'online' ? '' : label, sentence, lines, icon: 'cloud', since_ms: c.since_ms, on };
  }

  /** What happens to agents now, in one sentence. */
  function policy(data) {
    const c = data && data.connection, s = (data && data.settings) || {};
    const pick = data && data.local && data.local.pick;
    if (s.enabled === false) return 'Continuity is off: agents that lose their connection wait and retry.';
    if (!c || c.state === 'online') return 'Continuity is on: agents keep working if the connection drops.';
    const local = pick && pick.installed ? `a local model (${pick.tag})` : null;
    if (c.state === 'degraded' && !c.acts_offline) return 'Continuity is on: agents on a provider that cannot be reached continue on one that can.';
    return local ? `Continuity is on: agents continue on ${local}, or wait and retry.`
      : `Continuity is on, but no local model can take over${data && data.local && data.local.why_no_pick ? ` (${data.local.why_no_pick})` : ''}: agents wait and retry.`;
  }

  /** The side bar's line under the Agents title: empty while online. */
  function sidebar(data) {
    const c = connection(data);
    const waiting = ((data && data.waiting) || []).length;
    if (c.state === 'online' || c.state === 'unknown') return waiting ? `${plural(waiting, 'agent')} waiting to retry.` : '';
    return `${c.sentence}${waiting ? ` ${plural(waiting, 'agent')} waiting.` : ''}`;
  }

  /** The status bar item: text after the icon, and the tooltip. */
  function statusBar(data) {
    const c = connection(data);
    const waiting = ((data && data.waiting) || []).length;
    const text = [c.short, waiting ? `${waiting} waiting` : ''].filter(Boolean).join(' · ');
    return { icon: c.icon, text, tooltip: [...c.lines, waiting ? `${plural(waiting, 'agent')} waiting for ${waitingFor(data)}.` : ''].filter(Boolean).join('\n'), warn: c.state === 'offline', accessible: `Connection: ${c.label}${waiting ? `, ${plural(waiting, 'agent')} waiting` : ''}` };
  }

  function waitingFor(data) {
    const kinds = new Set(((data && data.waiting) || []).map(w => w.kind));
    return kinds.size === 1 && kinds.has('memory') ? 'memory' : kinds.has('memory') ? 'a connection or memory' : 'a connection';
  }

  /** The one Needs-you item for every waiting agent together. */
  function needsYou(data) {
    const waiting = (data && data.waiting) || [];
    if (!waiting.length) return undefined;
    const hours = (data.settings && data.settings.retryForHours) || 36;
    return { run_id: waiting[0].run_id, label: 'Waiting', title: `${plural(waiting.length, 'agent')} waiting for ${waitingFor(data)}`,
      detail: `${waiting.length === 1 ? 'It retries' : 'They retry'} on ${waiting.length === 1 ? 'its' : 'their'} own for up to ${hours} hours. Open one to use a local model or stop it.`, count: waiting.length };
  }

  /** "in 40 s", "in 3 min", "now". */
  function until(ms, now) {
    const s = Math.round((ms - now) / 1000);
    if (s <= 1) return 'now';
    if (s < 90) return `in ${s} s`;
    if (s < 5400) return `in ${Math.round(s / 60)} min`;
    return `in ${Math.round(s / 3600)} h`;
  }
  function since(ms, now) {
    const s = Math.max(0, Math.round((now - ms) / 1000));
    if (s < 90) return `${s} s`;
    if (s < 5400) return `${Math.round(s / 60)} min`;
    return `${Math.round(s / 3600)} h`;
  }

  /** The quiet card of a waiting agent: a title, one line, and its actions. */
  function waitCard(wait, data, now) {
    const memory = wait.kind === 'memory';
    const title = memory ? 'Waiting for memory' : 'Waiting for a connection';
    const line = memory ? 'The system ran short of memory, so the local model was unloaded. Your message is kept.'
      : `${wait.reason ? why(wait.reason).replace(/^./, m => m.toUpperCase()).replace(/\.$/, '') + '. ' : ''}Your message is kept.`;
    const next = wait.next_ms ? `Next check ${until(wait.next_ms, now)}` : '';
    const waited = wait.since_ms ? `waiting ${since(wait.since_ms, now)}` : '';
    const hours = (data && data.settings && data.settings.retryForHours) || 36;
    const actions = [];
    for (const o of wait.offers || []) {
      if (o.to === 'local') actions.push({ id: 'handoff', to: 'local', label: 'Use a local model now', detail: offerDetail(o), primary: true });
      else actions.push({ id: 'handoff', to: o.to, label: `Continue with ${o.label}`, detail: offerDetail(o), primary: true });
    }
    if (!memory) actions.push({ id: 'retry_now', label: 'Retry now' });
    actions.push({ id: 'stop', label: 'Stop' });
    return { title, line, note: wait.note || '', foot: [next, waited, `gives up after ${hours} hours`].filter(Boolean).join(' · '), actions, icon: 'cloud' };
  }

  /** What an offer means, with the difference in permission when there is one. */
  function offerDetail(o) {
    const who = o.to === 'local' ? `${o.label} (local, Ollama)` : `${o.label}${o.account ? `, account "${o.account}"` : ''}`;
    const mode = o.mode ? ` in ${MODE[o.mode] || o.mode}` : '';
    return o.difference ? `${who}${mode}. ${o.difference.replace(/^./, m => m.toUpperCase())}.` : `${who}${mode}.`;
  }

  /** The card a local or failed-over agent shows when the connection is back. */
  function backCard(offer) {
    const to = (offer.back_to && offer.back_to.label) || 'the first agent';
    if (offer.stay) return { title: 'Staying here', line: 'This agent stays on this model.', actions: [] };
    if (offer.at_next_turn) return { title: 'Back online', line: `Your next message continues with ${to}.`, actions: [{ id: 'stay', label: 'Stay here' }] };
    if (!offer.offer) return { title: 'Back online', line: '', actions: [] };
    return { title: 'Back online', line: `This agent can continue with ${to}, in the same worktree.`, actions: [{ id: 'handoff', to: 'back', label: `Switch back to ${to}`, primary: true }, { id: 'stay', label: 'Stay here' }] };
  }

  /** A local model's badge: what it says, its tone, and the full sentence for the tooltip. */
  function fit(model, budget) {
    const f = (model && model.fit) || {};
    const size = typeof f.bytes === 'number' ? `${gib(f.bytes)} GiB` : '';
    const room = budget && typeof budget.budget === 'number' ? `${gib(budget.budget)} GiB` : '';
    let badge, tone, detail;
    switch (f.status) {
      case 'fits':
        badge = `fits at ${Math.round(f.context / 1024)}k`; tone = f.context >= 32768 ? 'ok' : 'tight';
        detail = `${model.tag} fits at a ${Math.round(f.context / 1024)}k context: ${size}${room ? ` of ${room}` : ''}${f.already_loaded ? ', already loaded' : ''}.`;
        break;
      case 'too_big': badge = `too big${size ? ` · ${size}${room ? ` of ${room}` : ''}` : ''}`; tone = 'no'; detail = f.detail || `${model.tag} is over the memory budget.`; break;
      case 'not_installed': badge = `not installed · ${gib(f.download_bytes)} GiB download`; tone = 'off'; detail = `${model.tag} is not installed. ${f.may_download ? 'Overseer may download it.' : 'Downloads are off.'}`; break;
      case 'no_tools': badge = 'cannot call tools'; tone = 'no'; detail = f.detail || 'Ollama does not report tool calling for it.'; break;
      case 'too_small': badge = 'context too short'; tone = 'no'; detail = f.detail || ''; break;
      default: badge = 'not checked'; tone = 'off'; detail = '';
    }
    const mark = model.verified === 'passed' ? '' : model.verified === 'failed' ? 'failed its check' : 'unverified';
    if (mark) detail = `${detail} ${model.note ? why(model.note).replace(/^./, m => m.toUpperCase()) + '.' : mark.replace(/^./, m => m.toUpperCase()) + '.'}`.trim();
    detail = why(detail) || detail;
    return { badge, tone, mark, detail, usable: f.status === 'fits' };
  }

  /** Local models as a list to choose from: what fits first, then the rest, each with its badge. */
  function localChoices(data) {
    const local = (data && data.local) || {};
    const rank = m => ({ fits: 0, too_big: 2, not_installed: 3 }[m.fit && m.fit.status] ?? 4) + (m.verified === 'passed' ? 0 : 0.5);
    const models = [...(local.models || [])].sort((a, b) => rank(a) - rank(b) || (b.disk_bytes || 0) - (a.disk_bytes || 0));
    return { pick: local.pick || null, why_no_pick: local.why_no_pick || '', running: !!(local.ollama && local.ollama.running), ollama: (local.ollama && local.ollama.detail) || 'Ollama was not checked',
      budget: local.budget || null, models: models.map(m => ({ ...m, badge: fit(m, local.budget) })) };
  }

  /** Why a new agent cannot be started on this harness now, or undefined. */
  function harnessBlocked(harness, data) {
    const entry = ((data && data.new_agents && data.new_agents.harnesses) || []).find(h => h.harness === harness);
    return entry && entry.usable === false ? entry.why.replace(/^./, m => m.toUpperCase()) : undefined;
  }

  /** The first-use notice, shown once per machine. */
  function notice(data) {
    const s = (data && data.settings) || {};
    if (!data || !data.notice || !data.notice.show || s.enabled === false) return undefined;
    return {
      title: 'Continuity is on',
      more: 'What Continuity does, and the download and install settings',
      text: 'If the connection drops, agents keep working: on another provider that can be reached, or on a local model that fits this machine\'s memory. When neither is possible they wait and retry.',
      switches: [
        { setting: 'allowModelDownloads', label: 'Download local models', on: !!s.allowModelDownloads, detail: 'Off: only models that are installed are used.' },
        { setting: 'allowOllamaInstall', label: 'Install and start Ollama', on: !!s.allowOllamaInstall, detail: 'Off: Ollama is used only when it is already running.' },
      ],
      actions: [{ id: 'dismiss_notice', label: 'Got it', primary: true }, { id: 'toggle', label: 'Turn Continuity off' }],
    };
  }

  /** A download in words: "Downloading qwen3-coder:30b · 3.2 of 17.3 GiB". */
  function download(d) {
    const of = typeof d.total === 'number' && d.total > 0 ? `${gib(d.completed || 0)} of ${gib(d.total)} GiB` : '';
    const verb = { starting: 'Downloading', downloading: 'Downloading', done: 'Downloaded', cancelled: 'Download cancelled', failed: 'Download failed' }[d.status] || 'Downloading';
    return { text: [`${verb} ${d.tag}`, d.status === 'failed' ? why(d.reason) : of].filter(Boolean).join(' · '), percent: typeof d.percent === 'number' ? d.percent : undefined, active: d.status === 'downloading' || d.status === 'starting' };
  }

  /** The settings VS Code edits, with the ranges the daemon enforces. */
  const SETTINGS = {
    enabled: { type: 'boolean', default: true, text: 'Continuity: when an agent loses its connection, continue on another provider that can be reached or on a local model. Off: agents wait and retry.' },
    providerOrder: { type: 'array', items: { type: 'string', enum: ['openai', 'anthropic'] }, default: ['openai', 'anthropic'], text: 'Preference among providers that can be reached. Local models always come last.' },
    allowModelDownloads: { type: 'boolean', default: false, text: 'Let Overseer download local models from the Ollama registry when one is needed.' },
    allowOllamaInstall: { type: 'boolean', default: false, text: 'Let Overseer install Ollama (Homebrew, or the official archive after its signature verifies) and start it when nothing is running.' },
    prefetch: { type: 'boolean', default: false, text: 'While online and downloads are allowed, keep the best-fitting local model downloaded.' },
    ramCeilingPercent: { type: 'integer', minimum: 10, maximum: 50, default: 40, text: 'Share of total memory one local model may take. Never over 50.' },
    ramHeadroomGiB: { type: ['number', 'null'], minimum: 1, maximum: 1024, default: null, text: 'Free memory that must remain after a model loads, in GiB. Empty: 4 GiB or 10% of total, whichever is more.' },
    contextTarget: { type: 'integer', minimum: 8192, maximum: 1048576, default: 65536, text: 'Preferred context length for local models, in tokens.' },
    contextFloor: { type: 'integer', minimum: 8192, maximum: 262144, default: 16384, text: 'Shortest context Overseer runs a coding agent at, in tokens.' },
    preferredModels: { type: 'array', items: { type: 'string' }, default: [], text: 'Local models tried first, in this order (Ollama tags).' },
    allowUnverifiedModels: { type: 'boolean', default: false, text: 'Let automatic picks use installed models that call tools but have not passed Overseer\'s check.' },
    localHarness: { type: 'string', enum: ['opencode', 'codex'], default: 'opencode', text: 'The harness local models run in.' },
    returnOnline: { type: 'string', enum: ['offer', 'auto', 'stay'], default: 'offer', text: 'When the connection is back: offer to switch back, switch at the next message, or stay.' },
    retryCapSeconds: { type: 'integer', minimum: 5, maximum: 3600, default: 120, text: 'Longest wait between two retries, in seconds.' },
    retryForHours: { type: 'integer', minimum: 1, maximum: 36, default: 36, text: 'Stop waiting after this many hours. Never over 36.' },
    stallSeconds: { type: 'integer', minimum: 30, maximum: 3600, default: 90, text: 'Silence while offline before Overseer interrupts a turn, in seconds.' },
    probes: { type: 'boolean', default: true, text: 'Check reachability with small requests that carry no credentials.' },
    ollamaIdleMinutes: { type: 'integer', minimum: 1, maximum: 1440, default: 30, text: 'Stop an Ollama that Overseer started after this many idle minutes.' },
    registry: { type: 'string', default: '', text: 'Mirror for model downloads. Empty: Ollama\'s own registry.' },
  };

  /** The values to send to the daemon: only what differs from what it has. */
  function changed(wanted, current) {
    const out = {};
    for (const key of Object.keys(SETTINGS)) {
      if (!(key in wanted) || wanted[key] === undefined) continue;
      if (JSON.stringify(wanted[key]) !== JSON.stringify(current ? current[key] : undefined)) out[key] = wanted[key];
    }
    return out;
  }

  return { gib, plural, STATES, isWaiting, PROVIDER, HARNESS, MODE, connection, policy, sidebar, statusBar, needsYou, until, since, waitCard, offerDetail, backCard, fit, localChoices, harnessBlocked, notice, download, SETTINGS, changed };
});
