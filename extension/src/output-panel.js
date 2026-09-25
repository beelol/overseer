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
          entry.panel.webview.postMessage({ type: 'event', event, label: this.label(event.run_id) });
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

  async show(runId, { preserveFocus = true } = {}) {
    let entry = this.panels.get(runId);
    if (entry) { entry.panel.reveal(undefined, preserveFocus); return; }
    const panel = vscode.window.createWebviewPanel('overseer.output', 'Overseer run', { viewColumn: vscode.ViewColumn.Beside, preserveFocus }, { enableScripts: true, retainContextWhenHidden: true });
    entry = { panel, runIds: new Set([runId]) };
    this.panels.set(runId, entry);
    panel.onDidDispose(() => this.panels.delete(runId));
    panel.webview.onDidReceiveMessage(message => this.receive(runId, message).catch(error => {
      panel.webview.postMessage({ type: 'notice', message: error.message });
    }));
    const nonce = randomBytes(18).toString('base64');
    panel.webview.html = html(nonce, panel.webview.cspSource);
    for (const d of this.model.descendants(runId)) entry.runIds.add(d.id);
    this.pushRun(runId, entry);
    // History: each run's retained events, merged by the global sequence.
    const lists = await Promise.all([...entry.runIds].map(id => this.client.request('events.list', { run_id: id, after: 0, limit: 5000 })));
    const events = lists.flatMap(l => l.events).sort((a, b) => a.seq - b.seq);
    const truncated = lists.some(l => l.oldest_retained && events.length && l.events.some(e => e.kind === 'retention'));
    panel.webview.postMessage({ type: 'history', events: events.map(e => ({ event: e, label: this.label(e.run_id) })), truncated });
  }

  async receive(runId, message) {
    if (!message || typeof message !== 'object') return;
    if (!vscode.workspace.isTrusted && ['followUp', 'interrupt', 'permission'].includes(message.type)) throw new Error('Controlling agents requires a trusted workspace.');
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
    } else if (message.type === 'openReview') {
      await vscode.commands.executeCommand('overseer.openReview', runId);
    }
  }
}

function html(nonce, csp) {
  return `<!doctype html><html><head><meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${csp} 'unsafe-inline'; script-src 'nonce-${nonce}';">
<style>
body{font-family:var(--vscode-font-family);font-size:var(--vscode-font-size);color:var(--vscode-foreground);padding:0 12px 12px}
header{position:sticky;top:0;background:var(--vscode-editor-background);padding:8px 0;border-bottom:1px solid var(--vscode-panel-border);z-index:1}
h1{font-size:1.1em;margin:0 0 4px}
.meta{opacity:.8;font-size:.9em}
.status{font-weight:bold}
.caps{font-size:.85em;opacity:.85;margin-top:4px}
.caps summary{cursor:pointer}
#log{margin-top:8px}
.ev{padding:3px 0;border-bottom:1px solid var(--vscode-panel-border);white-space:pre-wrap;word-break:break-word}
.ev .who{opacity:.7;font-size:.85em;margin-right:6px}
.ev.kind-error{color:var(--vscode-errorForeground)}
.ev.kind-tool,.ev.kind-file_activity{opacity:.85;font-family:var(--vscode-editor-font-family);font-size:.9em}
.ev.kind-status,.ev.kind-turn_started,.ev.kind-turn_done,.ev.kind-session,.ev.kind-reattached{opacity:.7;font-style:italic}
.ev.kind-child{color:var(--vscode-charts-purple)}
.ev.kind-retention{color:var(--vscode-editorWarning-foreground)}
.perm{border:1px solid var(--vscode-editorWarning-foreground);padding:8px;margin:8px 0}
footer{position:sticky;bottom:0;background:var(--vscode-editor-background);padding-top:8px}
textarea{width:100%;box-sizing:border-box;min-height:48px;background:var(--vscode-input-background);color:var(--vscode-input-foreground);border:1px solid var(--vscode-input-border,transparent)}
button{background:var(--vscode-button-background);color:var(--vscode-button-foreground);border:none;padding:4px 10px;margin:4px 4px 0 0;cursor:pointer}
button:disabled{opacity:.5;cursor:default}
button.secondary{background:var(--vscode-button-secondaryBackground);color:var(--vscode-button-secondaryForeground)}
.why{font-size:.85em;opacity:.8}
pre.raw{max-height:300px;overflow:auto;font-size:.85em}
</style></head><body>
<header><h1 id="title">Run</h1><div class="meta"><span class="status" id="status"></span> <span id="meta"></span></div>
<div class="meta" id="ws"></div><div class="meta" id="children"></div>
<details class="caps"><summary>Capabilities (as reported by this adapter)</summary><div id="caps"></div></details>
<div><button id="review" class="secondary">Open Review</button><button id="raw" class="secondary">Raw output</button><button id="interrupt">Interrupt</button><span class="why" id="interrupt-why"></span></div></header>
<div id="perm"></div><div id="notice" class="why"></div><div id="log" role="log" aria-live="polite"></div><pre class="raw" id="rawout" hidden></pre>
<footer><textarea id="prompt" placeholder="Follow-up message to this run only"></textarea><button id="send">Send follow-up</button><span class="why" id="send-why"></span></footer>
<script nonce="${nonce}">
const vscode = acquireVsCodeApi();
const log = document.getElementById('log'); let run; let seen = new Set(); let stick = true;
window.addEventListener('scroll', () => { stick = (window.innerHeight + window.scrollY) >= document.body.scrollHeight - 40; });
function text(ev){ const p = ev.payload || {};
  switch(ev.kind){
    case 'output': return (p.role && p.role!=='assistant' ? '['+p.role+'] ' : '') + (p.text||'');
    case 'tool': return '⚙ ' + p.name + ': ' + (p.summary||'');
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
function add(ev, label){ if (seen.has(ev.seq)) return; seen.add(ev.seq);
  const d = document.createElement('div'); d.className = 'ev kind-' + ev.kind; d.dataset.seq = ev.seq;
  const who = document.createElement('span'); who.className = 'who'; who.textContent = label || ''; d.append(who, document.createTextNode(text(ev)));
  // Keep ordering by sequence even if an event arrives late.
  let after = null; for (const el of log.children) { if (Number(el.dataset.seq) > ev.seq) { after = el; break; } }
  log.insertBefore(d, after);
  while (log.children.length > 4000) log.firstChild.remove();
  if (stick) window.scrollTo(0, document.body.scrollHeight); }
function setRun(msg){ run = msg.run;
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
    box.append(t, pre, allow, deny); perm.append(box);
  } }
window.addEventListener('message', e => { const m = e.data;
  if (m.type === 'run') setRun(m);
  else if (m.type === 'history') { for (const x of m.events) add(x.event, x.label); }
  else if (m.type === 'event') add(m.event, m.label);
  else if (m.type === 'notice') document.getElementById('notice').textContent = m.message;
  else if (m.type === 'raw') { const r = document.getElementById('rawout'); r.hidden = false; r.textContent = (m.raw.truncated ? '[' + m.raw.note + ']\\n' : '') + m.raw.lines.map(l => '[' + l.s + '] ' + l.d).join('\\n'); } });
document.getElementById('send').onclick = () => { const t = document.getElementById('prompt'); if (!t.value.trim()) return; vscode.postMessage({ type: 'followUp', text: t.value }); t.value = ''; };
document.getElementById('interrupt').onclick = () => vscode.postMessage({ type: 'interrupt' });
document.getElementById('raw').onclick = () => vscode.postMessage({ type: 'raw' });
document.getElementById('review').onclick = () => vscode.postMessage({ type: 'openReview' });
</script></body></html>`;
}

module.exports = { OutputPanels };
