// Run output/control panel: streamed normalized events, follow-up, interrupt and
// permission answers. Messages only ever target this panel's run id.
const vscode = require('vscode');
const { randomBytes } = require('crypto');
const { ACTIVE } = require('./views');

class OutputPanels {
  constructor(context, client, model) {
    this.context = context; this.client = client; this.model = model;
    this.panels = new Map();
    client.on('event', event => {
      for (const entry of this.panels.values()) {
        if (entry.runIds.has(event.run_id) || (event.kind === 'child' && entry.runIds.has(event.run_id))) {
          if (event.kind === 'child' && event.payload?.child?.id) entry.runIds.add(event.payload.child.id);
          // Batched: a burst of thousands of events becomes a few messages, not one per event.
          (entry.queue ||= []).push({ event, label: this.label(event.run_id) });
          if (!entry.flush) entry.flush = setTimeout(() => { entry.flush = undefined; const items = entry.queue; entry.queue = []; entry.panel.webview.postMessage({ type: 'events', items }); }, 40);
        }
      }
    });
    model.onDidChange(() => { for (const [runId, entry] of this.panels) this.pushRun(runId, entry); });
  }

  label(runId) {
    const run = this.model.run(runId);
    if (!run) return runId;
    return run.parent_run_id ? `child · ${run.title}` : run.harness;
  }

  pushRun(runId, entry) {
    const run = this.model.run(runId);
    if (!run) return;
    for (const d of this.model.descendants(runId)) entry.runIds.add(d.id);
    const profile = run.profile_id && this.model.profile(run.profile_id);
    const ws = this.model.workspace(run.workspace_id);
    const turns = this.model.state.turns?.[runId] || [];
    entry.panel.title = `${run.harness}: ${run.title}`.slice(0, 60);
    entry.panel.webview.postMessage({ type: 'run', run, profile: profile?.name, workspace: ws, turns,
      trusted: vscode.workspace.isTrusted, active: ACTIVE.has(run.status),
      followUpSupported: !String(run.capabilities?.follow_up || '').startsWith('unsupported'),
      interruptSupported: !String(run.capabilities?.interrupt || '').startsWith('unsupported'),
      children: this.model.descendants(runId).map(c => ({ id: c.id, title: c.title, status: c.status, parent: c.parent_run_id, evidence: c.relation_source })) });
  }

  async show(runId, { preserveFocus = true, viewColumn } = {}) {
    const column = viewColumn || this.column?.();
    const entry = this.panels.get(runId);
    if (entry) { entry.panel.reveal(column, preserveFocus); return; }
    const panel = vscode.window.createWebviewPanel('overseer.output', 'Overseer run', { viewColumn: column || vscode.ViewColumn.Beside, preserveFocus }, { enableScripts: true, retainContextWhenHidden: true });
    await this.attach(runId, panel);
  }

  async attach(runId, panel) {
    const entry = { panel, runIds: new Set([runId]) };
    this.panels.set(runId, entry);
    panel.onDidDispose(() => { if (this.panels.get(runId) === entry) this.panels.delete(runId); });
    panel.webview.onDidReceiveMessage(message => this.receive(runId, message).catch(error => {
      panel.webview.postMessage({ type: 'notice', message: error.message });
    }));
    const nonce = randomBytes(18).toString('base64');
    const media = vscode.Uri.joinPath(this.context.extensionUri, 'media');
    panel.webview.options = { enableScripts: true, localResourceRoots: [media] };
    const asset = name => panel.webview.asWebviewUri(vscode.Uri.joinPath(media, name)).toString();
    panel.webview.html = html(nonce, panel.webview.cspSource, runId, { js: asset('conversation.js'), css: asset('conversation.css') });
    for (const d of this.model.descendants(runId)) entry.runIds.add(d.id);
    this.pushRun(runId, entry);
    // History: each run's retained events (paged; the newest matter most), merged by sequence.
    const fetchAll = async id => {
      const out = [];
      for (let after = 0, page = 0; page < 20; page++) {
        const list = await this.client.request('events.list', { run_id: id, after, limit: 5000 });
        out.push(...list.events);
        if (list.events.length < 5000) break;
        after = list.events[list.events.length - 1].seq;
      }
      return out;
    };
    const events = (await Promise.all([...entry.runIds].map(fetchAll))).flat().sort((a, b) => a.seq - b.seq);
    const truncated = events.some(e => e.kind === 'retention');
    panel.webview.postMessage({ type: 'history', events: events.map(e => ({ event: e, label: this.label(e.run_id) })), truncated });
  }

  /** Restores run panels after a window reload or VS Code restart (webview state holds the run id). */
  async deserializeWebviewPanel(panel, state) {
    const runId = state && typeof state.runId === 'string' ? state.runId : undefined;
    try {
      await this.client.waitConnected(20000);
      await this.model.refresh();
    } catch { /* explained below */ }
    const run = runId && this.model.run(runId);
    if (!run || this.panels.has(runId)) {
      if (run) { panel.dispose(); return; }
      panel.webview.options = { enableScripts: false };
      panel.title = 'Overseer run (unavailable)';
      panel.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';"></head><body style="font-family:var(--vscode-font-family);color:var(--vscode-foreground);padding:16px"><h2 style="font-size:1.1em">Run unavailable</h2><p>${this.client.connected ? 'This run is no longer in Overseer\'s records.' : 'Overseer\'s daemon is not reachable yet. Reopen the run from the Overseer view once it reconnects.'}</p></body></html>`;
      return;
    }
    await this.attach(runId, panel);
  }

  async receive(runId, message) {
    if (!message || typeof message !== 'object') return;
    if (!vscode.workspace.isTrusted && ['followUp', 'interrupt', 'permission', 'mergeBack'].includes(message.type)) throw new Error('Controlling agents requires a trusted workspace.');
    if (message.type === 'followUp') {
      const text = String(message.text || '').trim();
      if (!text) return;
      await this.client.request('run.follow_up', { run_id: runId, prompt: text });
      this.model.scheduleRefresh();
    } else if (message.type === 'interrupt') {
      await this.client.request('run.interrupt', { run_id: runId });
    } else if (message.type === 'permission') {
      await this.client.request('run.permission', { run_id: runId, request_id: String(message.request_id), allow: !!message.allow });
      this.model.scheduleRefresh();
    } else if (message.type === 'raw') {
      const raw = await this.client.request('run.raw_output', { run_id: runId, max_bytes: 512 * 1024 });
      this.panels.get(runId)?.panel.webview.postMessage({ type: 'raw', raw });
    } else if (message.type === 'mergeBack') {
      await vscode.commands.executeCommand('overseer.mergeBack', runId);
    } else if (message.type === 'openReview') {
      await vscode.commands.executeCommand('overseer.openReview', runId);
    } else if (message.type === 'openEdit') {
      // A file edit in the conversation: open the run's review at that file's edited hunk.
      await vscode.commands.executeCommand('overseer.openEdit', String(message.runId || runId), String(message.path || ''));
    }
  }
}

function html(nonce, csp, runId, media) {
  return `<!doctype html><html><head><meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${csp} 'unsafe-inline'; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${media.css}">
<style>
body{font-family:var(--vscode-font-family);font-size:var(--vscode-font-size);color:var(--vscode-foreground);background:var(--vscode-editor-background);padding:0 12px 12px}
header{position:sticky;top:0;background:var(--vscode-editor-background);padding:8px 0;border-bottom:1px solid var(--vscode-panel-border);z-index:1}
h1{font-size:1.1em;margin:0 0 4px}
.meta{opacity:.8;font-size:.9em}
.status{font-weight:bold}
.caps{font-size:.85em;opacity:.85;margin-top:4px}
.caps summary{cursor:pointer}
.tabs{display:flex;gap:4px;margin-top:6px}
.tabs button[aria-selected="true"]{background:var(--vscode-button-background);color:var(--vscode-button-foreground);outline:1px solid var(--vscode-contrastActiveBorder,transparent);outline-offset:1px;font-weight:600}
#log{margin-top:8px}
.ev{padding:3px 0;border-bottom:1px solid var(--vscode-panel-border);white-space:pre-wrap;word-break:break-word}
.ev .who{opacity:.7;font-size:.85em;margin-right:6px}
.ev.kind-error{color:var(--vscode-errorForeground)}
.ev.kind-tool,.ev.kind-tool_result,.ev.kind-file_activity{opacity:.85;font-family:var(--vscode-editor-font-family);font-size:.9em}
.ev.kind-status,.ev.kind-turn_started,.ev.kind-turn_done,.ev.kind-session,.ev.kind-reattached{opacity:.7;font-style:italic}
.ev.kind-child{color:var(--vscode-charts-purple)}
.ev.kind-retention{color:var(--vscode-editorWarning-foreground)}
.perm{border:1px solid var(--vscode-editorWarning-foreground);border-radius:6px;padding:8px;margin:8px 0;position:sticky;top:0;background:var(--vscode-editor-background);z-index:2}
.perm-input{max-height:140px;overflow:auto;font-size:.85em}
footer{position:sticky;bottom:0;background:var(--vscode-editor-background);padding-top:8px}
textarea{width:100%;box-sizing:border-box;min-height:48px;background:var(--vscode-input-background);color:var(--vscode-input-foreground);border:1px solid var(--vscode-input-border,transparent);border-radius:4px;font-family:inherit}
button{background:var(--vscode-button-background);color:var(--vscode-button-foreground);border:1px solid var(--vscode-button-border,var(--vscode-contrastBorder,transparent));border-radius:4px;padding:4px 10px;margin:4px 4px 0 0;cursor:pointer}
button:hover:not(:disabled){background:var(--vscode-button-hoverBackground)}
button.secondary:hover:not(:disabled){background:var(--vscode-button-secondaryHoverBackground)}
button:disabled{opacity:.5;cursor:default}
button.secondary{background:var(--vscode-button-secondaryBackground);color:var(--vscode-button-secondaryForeground)}
button:focus-visible{outline:1px solid var(--vscode-focusBorder);outline-offset:1px}
.why{font-size:.85em;opacity:.8}
pre.raw{max-height:300px;overflow:auto;font-size:.85em}
</style></head><body data-run-id="${runId.replace(/[^A-Za-z0-9_-]/g, '')}">
<header><h1 id="title">Run</h1><div class="meta"><span class="status" id="status"></span> <span id="meta"></span></div>
<div class="meta" id="ws"></div><div class="meta" id="children"></div>
<details class="caps"><summary>Capabilities (as reported by this adapter)</summary><div id="caps"></div></details>
<div><button id="review" class="secondary">Open Review</button><button id="raw" class="secondary">Raw output</button><button id="interrupt">Interrupt</button><button id="merge" class="secondary" title="Merge this run's branch back into its target branch (you review and confirm first)">Merge back…</button><span class="why" id="interrupt-why"></span></div>
<div class="tabs" role="tablist" aria-label="Run view"><button id="tab-conv" class="secondary" role="tab" aria-selected="true" aria-controls="conv">Conversation</button><button id="tab-log" class="secondary" role="tab" aria-selected="false" aria-controls="log">Event log</button></div></header>
<div id="perm"></div><div id="notice" class="why" role="status"></div><div id="conv" role="log" aria-live="polite" aria-label="Conversation"></div><div id="log" role="log" aria-label="Event log" hidden></div><pre class="raw" id="rawout" hidden></pre>
<footer><textarea id="prompt" placeholder="Follow-up message to this run only" aria-label="Follow-up message"></textarea><button id="send">Send follow-up</button><span class="why" id="send-why"></span></footer>
<script nonce="${nonce}" src="${media.js}"></script>
<script nonce="${nonce}">
const vscode = acquireVsCodeApi();
// Webview state (run id, view, scroll position, unsent follow-up draft) survives reloads and restarts.
const saved = vscode.getState() || {};
const same = saved.runId === document.body.dataset.runId;
const log = document.getElementById('log'); const convEl = document.getElementById('conv');
let run; let stick = same && saved.stick === false ? false : true; let restored = false; let view = same && saved.view === 'log' ? 'log' : 'conv';
const all = []; const labels = new Map(); let logBuilt = false; let scrollQueued = false;
const conversation = new window.OverseerConversation(convEl, { post: m => vscode.postMessage(m) });
const persist = () => vscode.setState({ runId: document.body.dataset.runId, scrollY: window.scrollY, stick, view, draft: document.getElementById('prompt').value });
if (same && saved.draft) document.getElementById('prompt').value = saved.draft;
persist();
window.addEventListener('scroll', () => { if (!restored) return; stick = (window.innerHeight + window.scrollY) >= document.body.scrollHeight - 40; persist(); });
document.getElementById('prompt').addEventListener('input', persist);
function text(ev){ const p = ev.payload || {};
  switch(ev.kind){
    case 'output': return (p.role && p.role!=='assistant' ? '['+p.role+'] ' : '') + (p.text||'');
    case 'tool': return '⚙ ' + p.name + ': ' + (p.summary||'');
    case 'tool_result': return '⚙ result ' + (p.id||'') + (p.status ? ' [' + p.status + ']' : '') + (p.output ? ': ' + String(p.output).slice(0, 300) : '');
    case 'file_activity': return '✎ ' + (p.paths||[]).join(', ') + ' (' + p.kind + ', ' + ev.confidence + ')';
    case 'error': return '✖ [' + p.class + '] ' + p.message;
    case 'status': return '— ' + p.status + (p.reason ? ': ' + p.reason : '');
    case 'child': return '↳ native child ' + (p.child && p.child.title) + ' (' + (p.evidence||'') + ')';
    case 'turn_started': return '▶ turn ' + (p.turn && p.turn.n) + ': ' + (p.turn && p.turn.prompt || '');
    case 'turn_done': return '■ turn ' + (p.ok ? 'completed' : 'failed') + (p.summary ? ': ' + p.summary : '');
    case 'permission': return '⚠ permission requested for ' + p.tool;
    case 'permission_answered': return (p.allow ? '✔ allowed ' : '✖ denied ') + p.request_id;
    case 'usage': return '∑ usage ' + JSON.stringify(p);
    case 'retention': return '… older history truncated by retention bound ' + JSON.stringify(p);
    case 'raw_unparsed': return '? unparsed (' + p.parser_version + '): ' + p.text;
    case 'session': return 'native session ' + p.native_id;
    case 'task_created': return 'task created in ' + (p.workspace ? p.workspace.kind + ' ' + p.workspace.path : '?') + (p.workspace && p.workspace.branch ? ' (' + p.workspace.branch + ')' : '');
    case 'interrupt_requested': return '⏹ interrupt requested';
    case 'reattached': return '↺ daemon restarted and reattached to this run';
    default: return ev.kind + ' ' + JSON.stringify(p);
  } }
function logLine(ev){
  const d = document.createElement('div'); d.className = 'ev kind-' + ev.kind; d.dataset.seq = ev.seq;
  const who = document.createElement('span'); who.className = 'who'; who.textContent = labels.get(ev.seq) || ''; d.append(who, document.createTextNode(text(ev)));
  let after = null; for (const el of log.children) { if (Number(el.dataset.seq) > ev.seq) { after = el; break; } }
  log.insertBefore(d, after);
  while (log.children.length > 4000) log.firstChild.remove(); }
function buildLog(){ if (logBuilt) return; logBuilt = true; log.replaceChildren(); for (const ev of all.slice(-4000)) logLine(ev); }
function add(ev, label){ if (labels.has(ev.seq)) return; labels.set(ev.seq, label || ''); all.push(ev); if (all.length > 20000) all.splice(0, all.length - 20000);
  conversation.add(ev); if (logBuilt) logLine(ev);
  if (stick && restored && !scrollQueued) { scrollQueued = true; requestAnimationFrame(() => { scrollQueued = false; if (stick) window.scrollTo(0, document.body.scrollHeight); }); } }
function show(v){ view = v; const conv = v === 'conv';
  convEl.hidden = !conv; log.hidden = conv; if (!conv) buildLog();
  document.getElementById('tab-conv').setAttribute('aria-selected', String(conv)); document.getElementById('tab-log').setAttribute('aria-selected', String(!conv)); persist(); }
function setRun(msg){ run = msg.run; conversation.setRun(msg);
  document.getElementById('title').textContent = run.title;
  document.getElementById('status').textContent = run.status.replace(/_/g,' ') + (run.exit_reason ? ' — ' + run.exit_reason : '');
  document.getElementById('meta').textContent = '· ' + run.harness + ' ' + (run.harness_version||'') + (msg.profile ? ' · account ' + msg.profile : '') + (run.model ? ' · ' + run.model : '') + ' · run ' + run.id + (run.native_id ? ' · native ' + run.native_id : '');
  document.getElementById('ws').textContent = msg.workspace ? 'Workspace: ' + msg.workspace.path + ' (' + msg.workspace.kind + (msg.workspace.branch ? ', ' + msg.workspace.branch : '') + ')' + (run.parent_run_id ? ' — shared with parent' : '') : '';
  document.getElementById('children').textContent = msg.children.length ? 'Native children: ' + msg.children.map(c => c.title + ' [' + c.status + ']').join(', ') : '';
  const caps = document.getElementById('caps'); caps.replaceChildren();
  for (const [k,v] of Object.entries(run.capabilities||{})) { const row = document.createElement('div'); row.textContent = k + ': ' + v; caps.append(row); }
  const child = !!run.parent_run_id;
  const interrupt = document.getElementById('interrupt'); interrupt.disabled = child || !msg.active || !msg.interruptSupported || !msg.trusted;
  document.getElementById('interrupt-why').textContent = child ? 'Native children are controlled through their parent run.' : !msg.trusted ? 'Requires a trusted workspace.' : !msg.active ? '' : !msg.interruptSupported ? 'Interrupt is not supported by this harness.' : '';
  const merge = document.getElementById('merge'); const worktree = msg.workspace && msg.workspace.kind === 'worktree';
  merge.hidden = child; merge.disabled = !worktree || msg.active || !msg.trusted;
  merge.title = !worktree ? 'This task works in the current checkout; there is no branch to merge back.' : msg.active ? 'Wait for the run to finish or interrupt it before merging back.' : !msg.trusted ? 'Requires a trusted workspace.' : "Merge this run's branch back into its target branch (you review and confirm first)";
  const send = document.getElementById('send'); const busy = msg.active && run.harness !== 'generic';
  send.disabled = child || !msg.followUpSupported || busy || !msg.trusted;
  document.getElementById('send-why').textContent = child ? 'Follow-ups go to the top-level run.' : !msg.followUpSupported ? 'This harness does not support follow-ups: ' + run.capabilities.follow_up : busy ? 'Wait for the current turn to finish or interrupt it.' : !msg.trusted ? 'Requires a trusted workspace.' : '';
  document.getElementById('prompt').disabled = send.disabled;
  const perm = document.getElementById('perm'); perm.replaceChildren();
  if (run.attention && run.attention.kind === 'permission') {
    const box = document.createElement('div'); box.className = 'perm';
    const t = document.createElement('div'); t.textContent = 'The agent is waiting for permission to use ' + run.attention.tool + ':';
    const pre = document.createElement('pre'); pre.textContent = JSON.stringify(run.attention.input, null, 2).slice(0, 4000);
    const allow = document.createElement('button'); allow.textContent = 'Allow once'; allow.onclick = () => vscode.postMessage({ type: 'permission', request_id: run.attention.request_id, allow: true });
    const deny = document.createElement('button'); deny.className = 'secondary'; deny.textContent = 'Deny'; deny.onclick = () => vscode.postMessage({ type: 'permission', request_id: run.attention.request_id, allow: false });
    pre.className = 'perm-input';
    box.append(t, allow, deny, pre); perm.append(box);
  } }
window.addEventListener('message', e => { const m = e.data;
  if (m.type === 'run') setRun(m);
  else if (m.type === 'history') { const t0 = performance.now(); for (const x of m.events) add(x.event, x.label); void document.body.offsetHeight;
    if (m.truncated) conversation.truncated('Older history was truncated by the retention bound. The raw output keeps the full stream.');
    document.body.dataset.historyMs = String(Math.round(performance.now() - t0)); document.body.dataset.historyEvents = String(m.events.length);
    if (!restored) { restored = true; if (!stick && same) window.scrollTo(0, saved.scrollY || 0); else window.scrollTo(0, document.body.scrollHeight); persist(); } }
  else if (m.type === 'event') add(m.event, m.label);
  else if (m.type === 'events') { for (const x of m.items) add(x.event, x.label); }
  else if (m.type === 'notice') document.getElementById('notice').textContent = m.message;
  else if (m.type === 'raw') { const r = document.getElementById('rawout'); r.hidden = false; r.textContent = (m.raw.truncated ? '[' + m.raw.note + ']\\n' : '') + m.raw.lines.map(l => '[' + l.s + '] ' + l.d).join('\\n'); } });
document.getElementById('send').onclick = () => { const t = document.getElementById('prompt'); if (!t.value.trim()) return; vscode.postMessage({ type: 'followUp', text: t.value }); t.value = ''; persist(); };
document.getElementById('interrupt').onclick = () => vscode.postMessage({ type: 'interrupt' });
document.getElementById('raw').onclick = () => vscode.postMessage({ type: 'raw' });
document.getElementById('review').onclick = () => vscode.postMessage({ type: 'openReview' });
document.getElementById('merge').onclick = () => vscode.postMessage({ type: 'mergeBack' });
document.getElementById('tab-conv').onclick = () => show('conv');
document.getElementById('tab-log').onclick = () => show('log');
show(view);
</script></body></html>`;
}

module.exports = { OutputPanels };
