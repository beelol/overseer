// Continuity in words (Gate L): the text of the status bar, the side bar, the waiting and
// back-online cards, the first-use notice and the fit badges, from the daemon's own answers.
// Run: node test/unit/continuity.js
const assert = require('assert');
const path = require('path');
const t = require(path.resolve(__dirname, '../../extension/media/continuity-text.js'));

let failures = 0;
const check = (name, fn) => { try { fn(); console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };
const G = 1024 ** 3;
const online = { state: 'online', reason: 'connected', acts_offline: false, system: { state: 'connected', detail: 'en0 is up with a default route' }, providers: { openai: { reachable: true, reason: 'answered' }, anthropic: { reachable: true, reason: 'answered' } } };
const degraded = { ...online, state: 'degraded', reason: 'OpenAI unreachable', providers: { openai: { reachable: false, reason: 'connect' }, anthropic: { reachable: true, reason: 'answered' } } };
const offline = { state: 'offline', reason: 'no network (system)', acts_offline: true, system: { state: 'none', detail: 'no network interface is connected' }, providers: {} };
const settings = { enabled: true, retryForHours: 36, allowModelDownloads: false, allowOllamaInstall: false };
const local = { pick: { tag: 'qwen3-coder:30b', context: 65536, installed: true }, budget: { budget: 51.2 * G }, ollama: { running: true, detail: 'Ollama is running' }, models: [
  { tag: 'qwen2.5-coder:14b', installed: true, disk_bytes: 9e9, verified: 'failed', note: 'failed its check with opencode: wrote its tool calls as text', fit: { status: 'fits', context: 32768, bytes: 15.4 * G } },
  { tag: 'qwen3.5:122b', installed: true, disk_bytes: 81e9, verified: 'unverified', note: 'not in the catalogue, so not verified with opencode', fit: { status: 'too_big', bytes: 77.2 * G, detail: 'qwen3.5:122b is too big to load: 77.2 GiB at a 16k context is over the budget of 51.2 GiB' } },
  { tag: 'qwen3-coder:30b', installed: true, disk_bytes: 18e9, verified: 'passed', note: null, fit: { status: 'fits', context: 65536, bytes: 24.3 * G } },
  { tag: 'qwen2.5-coder:7b', installed: false, disk_bytes: 4683087561, verified: 'failed', note: 'failed its check with opencode: said done without calling a tool', fit: { status: 'not_installed', download_bytes: 4683087561, may_download: false } },
] };

check('each connection state has its own words, and online is quiet', () => {
  const on = t.statusBar({ connection: online, settings, waiting: [] });
  assert.deepStrictEqual([on.text, on.warn, on.accessible], ['', false, 'Connection: Online']);
  assert.ok(on.tooltip.startsWith('Overseer is online.\nThe system says: en0 is up with a default route.\nOpenAI reachable · Claude reachable\nContinuity is on'), on.tooltip);
  const deg = t.statusBar({ connection: degraded, settings, waiting: [] });
  assert.deepStrictEqual([deg.text, deg.warn], ['OpenAI unreachable', false]);
  assert.ok(deg.tooltip.includes('OpenAI unreachable. The internet works.') && deg.tooltip.includes('OpenAI unreachable (connect) · Claude reachable') && deg.tooltip.includes('continue on one that can'), deg.tooltip);
  const off = t.statusBar({ connection: offline, settings, local, waiting: [{ run_id: 'r-1', kind: 'connection' }, { run_id: 'r-2', kind: 'connection' }] });
  assert.deepStrictEqual([off.text, off.warn, off.accessible], ['Offline · 2 waiting', true, 'Connection: Offline, 2 agents waiting']);
  assert.ok(off.tooltip.includes('Overseer is offline: no network (system).') && off.tooltip.includes('The system says: no network interface is connected.') && off.tooltip.includes('agents continue on a local model (qwen3-coder:30b), or wait and retry') && off.tooltip.endsWith('2 agents waiting for a connection.'), off.tooltip);
  assert.ok(!off.tooltip.includes('reachable ·'), 'offline does not list providers it could not ask');
});

check('no view says online while offline, and off is said as off', () => {
  for (const c of [offline, degraded]) for (const text of [t.sidebar({ connection: c, settings }), t.statusBar({ connection: c, settings }).tooltip, t.connection({ connection: c, settings }).label]) assert.ok(!/\bonline\b/i.test(text), text);
  assert.strictEqual(t.sidebar({ connection: online, settings, waiting: [] }), '');
  assert.strictEqual(t.sidebar({ connection: offline, settings, waiting: [{ kind: 'connection' }] }), 'Overseer is offline: no network (system). 1 agent waiting.');
  assert.strictEqual(t.policy({ connection: offline, settings: { enabled: false } }), 'Continuity is off: agents that lose their connection wait and retry.');
  assert.strictEqual(t.policy({ connection: offline, settings, local: { pick: null, why_no_pick: 'qwen3-coder:30b: not installed, and downloads are off' } }), 'Continuity is on, but no local model can take over (qwen3-coder:30b: not installed, and downloads are off): agents wait and retry.');
  assert.strictEqual(t.connection(undefined).state, 'unknown');
});

check('waiting agents are one Needs-you item', () => {
  assert.strictEqual(t.needsYou({ settings, waiting: [] }), undefined);
  const one = t.needsYou({ settings, waiting: [{ run_id: 'r-9', kind: 'connection' }] });
  assert.deepStrictEqual([one.run_id, one.label, one.title, one.count], ['r-9', 'Waiting', '1 agent waiting for a connection', 1]);
  assert.strictEqual(one.detail, 'It retries on its own for up to 36 hours. Open one to use a local model or stop it.');
  const more = t.needsYou({ settings, waiting: [{ run_id: 'a', kind: 'connection' }, { run_id: 'b', kind: 'memory' }, { run_id: 'c', kind: 'connection' }] });
  assert.deepStrictEqual([more.title, more.count], ['3 agents waiting for a connection or memory', 3]);
});

check('the waiting card says what is kept, when the next look is, and what can be done', () => {
  const now = 1_000_000;
  const card = t.waitCard({ kind: 'connection', reason: 'the connection to OpenAI failed: stream disconnected', since_ms: now - 125000, next_ms: now + 40000, note: null,
    offers: [{ to: 'local', label: 'qwen3-coder:30b', mode: 'acceptEdits', difference: null }] }, { settings }, now);
  assert.deepStrictEqual([card.title, card.line, card.foot], ['Waiting for a connection', 'The connection to OpenAI failed: stream disconnected. Your message is kept.', 'Next check in 40 s · waiting 2 min · gives up after 36 hours']);
  assert.deepStrictEqual(card.actions.map(a => a.label), ['Use a local model now', 'Retry now', 'Stop']);
  assert.strictEqual(card.actions[0].detail, 'qwen3-coder:30b (local, Ollama) in Accept edits.');
  const other = t.waitCard({ kind: 'connection', reason: 'x', offers: [{ to: 'openai', label: 'Codex', account: 'Work', mode: 'workspace-write', difference: 'Codex edits files and runs commands in its sandbox without asking first' }] }, { settings }, now);
  assert.deepStrictEqual([other.actions[0].label, other.actions[0].detail], ['Continue with Codex', 'Codex, account "Work" in Can edit. Codex edits files and runs commands in its sandbox without asking first.']);
  const memory = t.waitCard({ kind: 'memory', reason: 'critical pressure', offers: [] }, { settings }, now);
  assert.deepStrictEqual([memory.title, memory.actions.map(a => a.id)], ['Waiting for memory', ['stop']]);
  assert.deepStrictEqual([t.until(now + 500, now), t.until(now + 200000, now), t.until(now + 7.2e6, now), t.since(now - 3000, now)], ['now', 'in 3 min', 'in 2 h', '3 s']);
});

check('back online offers the way back, or says what will happen', () => {
  const offer = t.backCard({ offer: true, at_next_turn: false, back_to: { label: 'Codex' } });
  assert.deepStrictEqual([offer.title, offer.line, offer.actions.map(a => a.label)], ['Back online', 'This agent can continue with Codex, in the same worktree.', ['Switch back to Codex', 'Stay here']]);
  const auto = t.backCard({ offer: false, at_next_turn: true, back_to: { label: 'Claude Code' } });
  assert.deepStrictEqual([auto.line, auto.actions.map(a => a.id)], ['Your next message continues with Claude Code.', ['stay']]);
  assert.deepStrictEqual(t.backCard({ stay: true }).actions, []);
});

check('every local model has a badge, and a failed model says so', () => {
  const c = t.localChoices({ local });
  assert.deepStrictEqual(c.models.map(m => [m.tag, m.badge.badge, m.badge.tone, m.badge.mark, m.badge.usable]), [
    ['qwen3-coder:30b', 'fits at 64k', 'ok', '', true],
    ['qwen2.5-coder:14b', 'fits at 32k', 'ok', 'failed its check', true],
    ['qwen3.5:122b', 'too big · 77.2 GiB of 51.2 GiB', 'no', 'unverified', false],
    ['qwen2.5-coder:7b', 'not installed · 4.4 GiB download', 'off', 'failed its check', false],
  ]);
  assert.strictEqual(c.models[0].badge.detail, 'qwen3-coder:30b fits at a 64k context: 24.3 GiB of 51.2 GiB.');
  assert.ok(c.models[1].badge.detail.endsWith('Failed its check with OpenCode: wrote its tool calls as text.'), c.models[1].badge.detail);
  assert.ok(c.models[2].badge.detail.startsWith('qwen3.5:122b is too big to load: 77.2 GiB at a 16k context is over the budget of 51.2 GiB'));
  assert.strictEqual(t.fit({ tag: 'x', verified: 'passed', fit: { status: 'fits', context: 16384, bytes: 8 * G } }, { budget: 10 * G }).tone, 'tight');
  assert.deepStrictEqual([c.pick.tag, c.running], ['qwen3-coder:30b', true]);
});

check('offline, the online harnesses are blocked with the reason', () => {
  const data = { new_agents: { local_only: true, harnesses: [{ harness: 'codex', usable: false, why: 'offline: no network (system)' }, { harness: 'opencode-serve', usable: true, why: null }] } };
  assert.strictEqual(t.harnessBlocked('codex', data), 'Offline: no network (system)');
  assert.strictEqual(t.harnessBlocked('opencode-serve', data), undefined);
  assert.strictEqual(t.harnessBlocked('claude', {}), undefined);
});

check('the first-use notice shows the two settings as they are, once', () => {
  const n = t.notice({ notice: { show: true }, settings });
  assert.deepStrictEqual([n.title, n.switches.map(s => [s.setting, s.on]), n.actions.map(a => a.id)], ['Continuity is on', [['allowModelDownloads', false], ['allowOllamaInstall', false]], ['dismiss_notice', 'toggle']]);
  assert.strictEqual(t.notice({ notice: { show: false, shown_ms: 1 }, settings }), undefined);
  assert.strictEqual(t.notice({ notice: { show: true }, settings: { enabled: false } }), undefined);
});

check('a download reads as progress', () => {
  assert.deepStrictEqual(t.download({ tag: 'qwen3-coder:30b', status: 'downloading', completed: 3.2 * G, total: 17.3 * G, percent: 18 }), { text: 'Downloading qwen3-coder:30b · 3.2 of 17.3 GiB', percent: 18, active: true });
  assert.deepStrictEqual(t.download({ tag: 'x', status: 'failed', reason: 'pull model manifest: file does not exist', total: 0 }), { text: 'Download failed x · pull model manifest: file does not exist', percent: undefined, active: false });
});

check('only what differs is sent to the daemon', () => {
  assert.deepStrictEqual(t.changed({ enabled: true, ramCeilingPercent: 45, providerOrder: ['openai', 'anthropic'], unknown: 1 }, { enabled: true, ramCeilingPercent: 40, providerOrder: ['openai', 'anthropic'] }), { ramCeilingPercent: 45 });
  assert.deepStrictEqual(t.changed({ retryForHours: undefined }, {}), {});
  assert.strictEqual(Object.keys(t.SETTINGS).length, 19);
});

check('the manifest offers every setting with the daemon\'s ranges', () => {
  const pkg = require(path.resolve(__dirname, '../../extension/package.json'));
  const props = pkg.contributes.configuration.properties;
  for (const [key, s] of Object.entries(t.SETTINGS)) {
    const p = props[`overseer.continuity.${key}`];
    assert.ok(p, `overseer.continuity.${key} is not in package.json`);
    assert.deepStrictEqual([p.type, p.default, p.minimum, p.maximum, p.enum], [s.type, s.default, s.minimum, s.maximum, s.enum], key);
    assert.ok((p.markdownDescription || p.description || '').length > 20, key);
  }
  assert.deepStrictEqual([props['overseer.continuity.ramCeilingPercent'].maximum, props['overseer.continuity.retryForHours'].maximum], [50, 36]);
});

console.log(failures ? `${failures} check(s) failed` : 'all Continuity text checks pass');
process.exit(failures ? 1 : 0);
