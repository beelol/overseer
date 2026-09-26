// Chat view (AC-55): one agent's conversation with a quiet header, the conversation column, a
// needs-you bar, a changes bar and the composer. Shared by the Overseer dashboard and run panels.
// Everything rare (merge back, pull request, raw output, event log, details, cleanup) lives in the
// … menu. IDs used by tests are stable: #title #status #prompt #send #interrupt #review #more
// #merge #pr #raw #tab-conv #tab-log #perm #changes #conv #log #rawout.
(function () {
  const ui = window.OverseerUI;
  const el = ui.el;

  class Chat {
    /** opts: { post(msg), mode: 'panel' | 'dashboard', onState?(state) } */
    constructor(root, opts) {
      this.root = root; this.opts = opts; this.post = opts.post;
      this.runId = undefined; this.msg = undefined; this.view = 'conv'; this.stick = true; this.drafts = new Map();
      root.classList.add('chat'); root.dataset.auditView = 'chat';
      this.build();
    }

    build() {
      const head = el('header', 'chat-head');
      this.statusEl = el('span', 'status'); this.statusEl.id = 'status';
      const titles = el('div', 'chat-titles');
      this.titleEl = el('h1', null, ''); this.titleEl.id = 'title';
      this.metaEl = el('div', 'chat-meta'); this.metaEl.id = 'meta';
      titles.append(this.titleEl, this.metaEl);
      const actions = el('div', 'chat-actions');
      this.reviewBtn = ui.iconButton('diff-multiple', 'Review changes', { action: 'review' }); this.reviewBtn.id = 'review';
      this.filesBtn = ui.iconButton('list-tree', 'Files', { action: 'files', pressed: false }); this.filesBtn.id = 'files-toggle';
      this.stopBtn = ui.iconButton('debug-stop', 'Stop', { action: 'interrupt', shortcut: '⌘.' }); this.stopBtn.id = 'interrupt';
      this.moreBtn = ui.iconButton('ellipsis', 'More actions', { action: 'more' }); this.moreBtn.id = 'more'; this.moreBtn.setAttribute('aria-haspopup', 'menu');
      if (this.opts.mode !== 'dashboard') this.filesBtn.hidden = true;
      actions.append(this.stopBtn, this.reviewBtn, this.filesBtn, this.moreBtn);
      head.append(this.statusEl, titles, actions);

      this.scroll = el('div', 'chat-scroll'); this.scroll.id = 'scroll';
      const col = el('div', 'chat-column');
      this.details = el('div', 'details-panel'); this.details.hidden = true; this.details.id = 'details';
      this.convEl = el('div'); this.convEl.id = 'conv'; this.convEl.setAttribute('role', 'log'); this.convEl.setAttribute('aria-live', 'polite'); this.convEl.setAttribute('aria-label', 'Conversation');
      this.logEl = el('div'); this.logEl.id = 'log'; this.logEl.hidden = true; this.logEl.setAttribute('role', 'log'); this.logEl.setAttribute('aria-label', 'Event log');
      this.rawEl = el('pre', 'code raw-out'); this.rawEl.id = 'rawout'; this.rawEl.hidden = true;
      col.append(this.details, this.convEl, this.logEl, this.rawEl);
      this.scroll.append(col);
      this.jump = el('button', 'jump'); this.jump.type = 'button'; this.jump.hidden = true; this.jump.id = 'jump'; this.jump.append(ui.icon('arrow-down', 'sm'), el('span', null, 'Latest'));
      this.jump.setAttribute('aria-label', 'Jump to the latest message');

      const bottom = el('div', 'chat-bottom'); const inner = el('div', 'chat-bottom-inner');
      this.permBar = el('div', 'needs-bar'); this.permBar.id = 'perm'; this.permBar.hidden = true; this.permBar.setAttribute('role', 'alert');
      this.changesBar = el('button', 'changes-bar'); this.changesBar.id = 'changes'; this.changesBar.type = 'button'; this.changesBar.hidden = true;
      this.noticeEl = el('div', 'composer-note'); this.noticeEl.id = 'notice'; this.noticeEl.setAttribute('role', 'status');
      const composer = el('div', 'composer inline');
      this.prompt = el('textarea'); this.prompt.id = 'prompt'; this.prompt.rows = 1; this.prompt.setAttribute('aria-label', 'Message to this agent');
      const row = el('div', 'composer-row'); this.chips = el('div', 'chips');
      this.sendBtn = ui.iconButton('arrow-up', 'Send', { cls: 'primary send', shortcut: 'Enter' }); this.sendBtn.id = 'send';
      const tools = el('div', 'composer-tools'); this.tray = el('div', 'composer-tray'); this.tray.hidden = true;
      row.append(this.chips, this.sendBtn);
      composer.append(tools, this.prompt, row);
      this.queuedEl = el('div', 'queued'); this.queuedEl.hidden = true; this.queuedEl.id = 'queued';
      this.tools = window.OverseerPromptTools.create(this.prompt, tools, this.tray, { post: m => this.post(m), harness: () => this.msg?.run.harness, target: () => this.msg?.workspace ? { workspace_id: this.msg.workspace.id } : null,
        notice: t => this.notice(t), onChange: () => {} });
      this.why = el('div', 'composer-note'); this.why.id = 'send-why';
      inner.append(this.permBar, this.changesBar, this.queuedEl, this.noticeEl, this.tray, composer, this.why);
      bottom.append(inner);
      this.root.replaceChildren(head, this.scroll, this.jump, bottom);

      this.stopBtn.addEventListener('click', () => this.post({ type: 'interrupt' }));
      this.reviewBtn.addEventListener('click', () => this.post({ type: 'openReview' }));
      this.changesBar.addEventListener('click', () => this.post({ type: 'openReview' }));
      this.filesBtn.addEventListener('click', () => this.opts.onFiles && this.opts.onFiles(this.filesBtn));
      this.moreBtn.addEventListener('click', () => this.menu());
      this.sendBtn.addEventListener('click', e => this.send(e.altKey ? 'interrupt' : 'queue'));
      this.jump.addEventListener('click', () => { this.stick = true; this.toBottom(); });
      this.prompt.addEventListener('keydown', e => {
        if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); this.send(e.altKey ? 'interrupt' : 'queue'); }
        else if (e.key === '.' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); if (!this.stopBtn.hidden) this.post({ type: 'interrupt' }); }
      });
      this.prompt.addEventListener('input', () => { this.grow(); if (this.runId) this.drafts.set(this.runId, this.prompt.value); this.opts.onState && this.opts.onState(); });
      this.scroll.addEventListener('scroll', () => {
        const atEnd = this.scroll.scrollTop + this.scroll.clientHeight >= this.scroll.scrollHeight - 40;
        this.stick = atEnd; this.jump.hidden = atEnd || !this.runId;
        this.opts.onState && this.opts.onState();
      });
    }

    grow() { this.prompt.style.height = 'auto'; this.prompt.style.height = Math.min(240, this.prompt.scrollHeight) + 'px'; }
    toBottom() { this.scroll.scrollTop = this.scroll.scrollHeight; this.jump.hidden = true; }
    scheduleBottom() { if (!this.stick || this.queued) return; this.queued = true; requestAnimationFrame(() => { this.queued = false; if (this.stick) this.toBottom(); }); }

    /** Starts showing a run (clears the previous one). */
    reset(runId) {
      if (this.runId === runId) return;
      if (this.runId) this.drafts.set(this.runId, this.prompt.value);
      this.runId = runId; this.all = []; this.labels = new Map(); this.logBuilt = false; this.stick = true; this.restored = false;
      this.conversation = new window.OverseerConversation(this.convEl, { post: m => this.post(m) });
      this.logEl.replaceChildren(); this.rawEl.hidden = true; this.details.hidden = true;
      this.prompt.value = this.drafts.get(runId) || ''; this.grow();
      this.changesBar.hidden = true; this.permBar.hidden = true; this.noticeEl.textContent = '';
      this.show('conv');
    }

    setRun(msg) {
      if (msg.run.id !== this.runId) this.reset(msg.run.id);
      this.msg = msg;
      const run = msg.run, child = !!run.parent_run_id;
      this.conversation.setRun(msg);
      this.titleEl.textContent = run.title || ui.firstLine(msg.prompt) || 'Agent';
      this.titleEl.title = run.title || '';
      this.statusEl.replaceWith(this.statusEl = Object.assign(ui.status(run.status, run.attention?.kind), { id: 'status' }));
      this.statusEl.dataset.text = run.status;
      // One quiet meta line: account (with logo) · model · branch. Everything else is in Details.
      const items = [];
      const acct = el('span', 'meta-item'); acct.append(ui.harnessMark(run.harness, 13), el('span', null, msg.profile || ui.HARNESS[run.harness] || run.harness));
      acct.title = `${ui.HARNESS[run.harness] || run.harness}${run.harness_version ? ' ' + run.harness_version : ''}${msg.profile ? ' · ' + msg.profile : ''}`;
      items.push(acct);
      if (run.model) { const m = el('span', 'meta-item', run.model); m.title = 'Model'; items.push(m); }
      if (msg.workspace) {
        const w = el('span', 'meta-item');
        w.append(ui.icon(msg.workspace.kind === 'worktree' ? 'git-branch' : 'repo', 'xs'), el('span', 'ellipsis', msg.workspace.kind === 'worktree' ? ui.basename(msg.workspace.branch) : 'current checkout'));
        w.title = `${msg.workspace.kind === 'worktree' ? msg.workspace.branch : 'Current checkout'}\n${msg.workspace.path}`;
        items.push(w);
      }
      this.metaEl.replaceChildren(...items.flatMap((x, i) => (i ? [el('span', 'sep', '·'), x] : [x])));
      if (run.exit_reason && /failed|interrupted|disconnected/.test(run.status)) this.metaEl.title = run.exit_reason; else this.metaEl.removeAttribute('title');

      this.stopBtn.hidden = child || !msg.active || !msg.interruptSupported || !msg.trusted;
      const worktree = msg.workspace && msg.workspace.kind === 'worktree';
      this.reviewBtn.hidden = child;
      this.why.textContent = '';
      const busy = msg.active && run.harness !== 'generic';
      this.busy = busy;
      const canSend = !child && msg.followUpSupported && msg.trusted;
      this.sendBtn.disabled = !canSend; this.prompt.disabled = !canSend;
      this.prompt.placeholder = child ? 'Sub-agents are steered through their parent' : !msg.trusted ? 'Trust this workspace to talk to agents'
        : !msg.followUpSupported ? `${ui.HARNESS[run.harness] || run.harness} does not take follow-ups` : busy ? 'Message for when it finishes · ⌥⏎ stops and sends' : 'Reply…  (@ to mention a file)';
      this.sendBtn.replaceChildren(ui.icon(busy ? 'history' : 'arrow-up'));
      this.sendBtn.title = !canSend ? this.prompt.placeholder : busy ? 'Send when this turn ends (Enter) · stop and send now (⌥Enter)' : 'Send (Enter)';
      this.sendBtn.setAttribute('aria-label', busy ? 'Queue message' : 'Send');
      this.tools.refresh();
      this.renderQueued(msg.queued);
      this.worktree = worktree; this.child = child;
      this.renderDetails();
      this.renderPermBar();
      this.opts.onState && this.opts.onState();
    }

    renderPermBar() {
      const run = this.msg && this.msg.run;
      const att = run && run.attention && run.attention.kind === 'permission' ? run.attention : undefined;
      this.permBar.hidden = !att;
      if (!att) { this.permBar.replaceChildren(); return; }
      const d = window.OverseerConversation.describe(att.tool, att.input);
      const text = el('span', 'needs-text', `Allow ${d.pending || d.verb} ${d.target || ''}?`.replace(/\s+\?$/, '?'));
      text.title = d.full || att.tool;
      const allow = el('button', 'btn primary sm', 'Allow once'); allow.type = 'button'; allow.dataset.permission = 'allow';
      allow.addEventListener('click', () => this.post({ type: 'permission', request_id: att.request_id, allow: true }));
      const deny = el('button', 'btn sm', 'Deny'); deny.type = 'button'; deny.dataset.permission = 'deny';
      deny.addEventListener('click', () => this.post({ type: 'permission', request_id: att.request_id, allow: false }));
      this.permBar.replaceChildren(ui.icon('shield', 'sm'), text, allow, deny);
    }

    renderDetails() {
      const run = this.msg.run, ws = this.msg.workspace;
      const dl = el('dl');
      const row = (k, v, wrap) => { if (!v) return; const dd = el('dd', wrap ? 'wrap' : null, v); dd.title = v; dl.append(el('dt', null, k), dd); };
      row('Status', ui.statusText(run.status) + (run.exit_reason ? ` (${run.exit_reason})` : ''), true);
      row('Harness', `${ui.HARNESS[run.harness] || run.harness} ${run.harness_version || ''}`.trim());
      row('Account', this.msg.profile);
      row('Model', run.model);
      row('Branch', ws && ws.branch);
      row('Worktree', ws && ws.path);
      row('Run', run.id);
      row('Session', run.native_id);
      const caps = Object.entries(run.capabilities || {}).map(([k, v]) => `${k.replace(/_/g, ' ')}: ${String(v).split(/[;(]/)[0].trim()}`).join('\n');
      row('Capabilities', caps, true);
      this.details.replaceChildren(dl);
    }

    menu() {
      const m = this.msg; if (!m) return;
      const blocked = m.active ? 'Wait for the agent to finish or stop it first' : !m.trusted ? 'Requires a trusted workspace' : !this.worktree ? 'This task works in the current checkout' : '';
      const items = [];
      // Narrow (beside a diff): the header's Review and Files buttons are in this menu instead.
      if (window.innerWidth <= 480) {
        items.push({ id: 'review-menu', label: 'Review changes', icon: 'diff-multiple', run: () => this.post({ type: 'openReview' }) });
        if (this.opts.mode === 'dashboard') items.push({ id: 'files-menu', label: 'Files', icon: 'list-tree', run: () => this.opts.onFiles?.() });
        items.push('sep');
      }
      if (!this.child) {
        items.push({ id: 'merge', label: 'Merge back…', icon: 'git-merge', disabled: !!blocked, why: blocked, run: () => this.post({ type: 'mergeBack' }) });
        items.push({ id: 'pr', label: 'Open pull request…', logo: window.OverseerLogos && window.OverseerLogos.logo('github', { size: 14 }), disabled: !!blocked, why: blocked, run: () => this.post({ type: 'openPullRequest' }) });
        items.push('sep');
      }
      items.push({ id: this.view === 'log' ? 'tab-conv' : 'tab-log', label: this.view === 'log' ? 'Show conversation' : 'Show event log', icon: this.view === 'log' ? 'comment-discussion' : 'list-flat', run: () => this.show(this.view === 'log' ? 'conv' : 'log') });
      items.push({ id: 'raw', label: 'Raw output', icon: 'output', run: () => this.post({ type: 'raw' }) });
      items.push({ id: 'details-toggle', label: this.details.hidden ? 'Details' : 'Hide details', icon: 'info', run: () => { this.details.hidden = !this.details.hidden; if (!this.details.hidden) this.scroll.scrollTop = 0; } });
      items.push({ label: 'Copy run ID', icon: 'copy', run: () => ui.copy(this.post, m.run.id) });
      if (!this.child && !m.active && m.taskId) items.push({ id: 'archive', label: m.archived ? 'Restore from archive' : 'Archive', icon: m.archived ? 'discard' : 'archive', run: () => this.post({ type: 'archive', taskId: m.taskId, archived: !m.archived }) });
      if (this.opts.mode === 'dashboard') items.push({ label: 'Open in its own tab', icon: 'link-external', run: () => this.post({ type: 'openPanel' }) });
      if (!this.child && this.worktree) { items.push('sep'); items.push({ id: 'cleanup', label: 'Remove worktree…', icon: 'trash', danger: true, disabled: m.active, why: m.active ? 'Stop the agent first' : '', run: () => this.post({ type: 'cleanup' }) }); }
      ui.menu(this.moreBtn, items, { align: 'end', label: 'More actions' });
    }

    send(how) {
      const text = this.prompt.value.trim();
      if (!text || this.sendBtn.disabled) return;
      const { prompt, options } = this.tools.take();
      if (this.busy) this.post({ type: 'steer', text: prompt, options, how });
      else this.post({ type: 'followUp', text: prompt, options });
      this.prompt.value = ''; this.drafts.delete(this.runId); this.grow(); this.stick = true;
      this.opts.onState && this.opts.onState();
    }

    renderQueued(q) {
      this.queuedEl.hidden = !q;
      if (!q) { this.queuedEl.replaceChildren(); return; }
      const cancel = el('button', 'link', 'Cancel'); cancel.type = 'button'; cancel.addEventListener('click', () => this.post({ type: 'steer', how: 'cancel' }));
      const text = el('span', 'queued-text', ui.firstLine(q.text, 80)); text.title = q.text;
      this.queuedEl.replaceChildren(ui.icon(q.how === 'interrupt' ? 'debug-stop' : 'history', 'sm'), el('span', 'muted', q.how === 'interrupt' ? 'Stopping, then sending' : 'Queued'), text, cancel);
    }
    mentionFiles(m) { this.tools.files(m); }

    show(v) {
      this.view = v; const conv = v === 'conv';
      this.convEl.hidden = !conv; this.logEl.hidden = conv;
      if (!conv) this.buildLog();
      this.opts.onState && this.opts.onState();
    }

    add(ev, label) {
      if (this.labels.has(ev.seq)) return;
      this.labels.set(ev.seq, label || '');
      this.all.push(ev); if (this.all.length > 20000) this.all.splice(0, this.all.length - 20000);
      this.conversation.add(ev);
      if (this.logBuilt) this.logLine(ev);
      if (this.restored) this.scheduleBottom();
    }
    history(events, truncated, restore) {
      const t0 = performance.now();
      for (const x of events) this.add(x.event, x.label);
      if (truncated) this.conversation.truncated('Older history was trimmed. Raw output keeps everything.');
      this.root.dataset.historyMs = String(Math.round(performance.now() - t0)); this.root.dataset.historyEvents = String(events.length);
      document.body.dataset.historyMs = this.root.dataset.historyMs; document.body.dataset.historyEvents = this.root.dataset.historyEvents;
      this.restored = true;
      if (restore && restore.stick === false && typeof restore.scrollTop === 'number') { this.stick = false; this.scroll.scrollTop = restore.scrollTop; this.jump.hidden = false; }
      else this.toBottom();
    }
    events(items) { for (const x of items) this.add(x.event, x.label); }
    notice(text) { this.noticeEl.textContent = text || ''; this.noticeEl.classList.toggle('error', !!text); }
    raw(raw) { this.rawEl.hidden = false; this.rawEl.textContent = (raw.truncated ? `[${raw.note}]\n` : '') + raw.lines.map(l => `[${l.s}] ${l.d}`).join('\n'); this.rawEl.scrollIntoView({ block: 'start' }); }
    changes(c) {
      const n = c && c.files || 0;
      this.changesBar.hidden = !n || this.child;
      if (!n) return;
      const names = (c.names || []).slice(0, 3).join(', ') + ((c.names || []).length > 3 ? ', …' : '');
      this.changesBar.replaceChildren(ui.icon('diff', 'sm'), el('span', null, `${n} file${n === 1 ? '' : 's'}`), ...(c.added !== undefined ? [el('span', 'add', `+${c.added}`), el('span', 'del', `−${c.removed}`)] : []),
        el('span', 'changes-files', names), ui.icon('chevron-right', 'sm'));
      this.changesBar.title = 'Review changes\n' + (c.names || []).join('\n');
    }

    buildLog() {
      if (this.logBuilt) return; this.logBuilt = true; this.logEl.replaceChildren();
      for (const ev of this.all.slice(-4000)) this.logLine(ev);
    }
    logLine(ev) {
      const d = el('div', 'ev kind-' + ev.kind); d.dataset.seq = ev.seq;
      d.append(el('span', 'who', ev.kind), document.createTextNode(logText(ev)));
      let after = null; for (const x of this.logEl.children) { if (Number(x.dataset.seq) > ev.seq) { after = x; break; } }
      this.logEl.insertBefore(d, after);
      while (this.logEl.children.length > 4000) this.logEl.firstChild.remove();
    }
    state() { return { runId: this.runId, view: this.view, stick: this.stick, scrollTop: this.scroll.scrollTop, draft: this.prompt.value }; }
  }

  function logText(ev) {
    const p = ev.payload || {};
    switch (ev.kind) {
      case 'output': return (p.role && p.role !== 'assistant' ? `[${p.role}] ` : '') + (p.text || '');
      case 'tool': return `${p.name}: ${p.summary || ''}`;
      case 'tool_result': return `${p.id || ''}${p.status ? ' [' + p.status + ']' : ''}${p.output ? ': ' + String(p.output).slice(0, 300) : ''}`;
      case 'file_activity': return `${(p.paths || []).join(', ')} (${p.kind}, ${ev.confidence})`;
      case 'error': return `[${p.class}] ${p.message}`;
      case 'status': return `${p.status}${p.reason ? ': ' + p.reason : ''}`;
      case 'turn_started': return `turn ${p.turn && p.turn.n}: ${(p.turn && p.turn.prompt) || ''}`;
      case 'turn_done': return `turn ${p.ok ? 'completed' : 'failed'}${p.summary ? ': ' + p.summary : ''}`;
      default: return JSON.stringify(p);
    }
  }

  window.OverseerChat = Chat;
})();
