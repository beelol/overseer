// Conversation renderer for Overseer chats (AC-43, AC-55): normalized daemon events become a
// calm chat. Your prompts are bubbles; agent replies are Markdown; tool calls are one-line rows
// ("Read README.md ✓", "Ran npm test ✓") and consecutive ones fold into a single line; file edits
// open the review at the hunk; permission requests are readable cards with Allow / Deny; native
// children nest under the tool call that started them; each turn ends with a quiet footer.
// Used by the Overseer dashboard, run panels and grid tiles. No innerHTML except sanitized Markdown.
(function () {
  const ui = window.OverseerUI;
  const el = ui.el;
  const SPAWN_TOOLS = /^(Agent|Task|task|collab:spawn_agent|spawn_agent)$/;
  const QUIET = new Set(['session', 'task_created', 'reattached', 'interrupt_requested', 'workspace_removed', 'background_notice', 'daemon_stopping', 'status', 'usage']);

  function parseInput(v) {
    if (v && typeof v === 'object') return v;
    const s = String(v || '');
    try { const j = JSON.parse(s); if (j && typeof j === 'object') return j; } catch { /* truncated or not JSON */ }
    const out = {};
    for (const k of ['file_path', 'path', 'command', 'pattern', 'description', 'url', 'query', 'prompt', 'notebook_path']) {
      const m = new RegExp(`"${k}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`).exec(s);
      if (m) out[k] = m[1].replace(/\\n/g, '\n').replace(/\\"/g, '"');
    }
    return out;
  }
  const lines = t => (t ? String(t).split('\n').length - (String(t).endsWith('\n') ? 1 : 0) : 0);
  const host = u => { try { return new URL(u).host; } catch { return u; } };

  /** Icon, verb and target for a tool call, from its name and input. */
  function describe(name, input, summary) {
    const i = parseInput(input !== undefined && input !== null ? input : summary);
    const file = i.file_path || i.notebook_path || i.path;
    const n = String(name || 'tool');
    switch (n) {
      case 'Read': return { icon: 'file', verb: 'Read', target: ui.basename(file), full: file };
      case 'Write': return { icon: 'new-file', verb: 'Created', pending: 'Create', target: ui.basename(file), full: file, added: lines(i.content) };
      case 'Edit': case 'MultiEdit': case 'NotebookEdit': {
        const edits = Array.isArray(i.edits) ? i.edits : [i];
        return { icon: 'edit', verb: 'Edited', pending: 'Edit', target: ui.basename(file), full: file, added: edits.reduce((a, e) => a + lines(e.new_string || e.new_source), 0), removed: edits.reduce((a, e) => a + lines(e.old_string), 0) };
      }
      case 'Grep': return { icon: 'search', verb: 'Searched', target: i.pattern ? `“${i.pattern}”` : '', full: i.pattern };
      case 'Glob': return { icon: 'search', verb: 'Found files', target: i.pattern || '', full: i.pattern };
      case 'LS': return { icon: 'folder', verb: 'Listed', target: ui.basename(i.path), full: i.path };
      case 'Bash': case 'shell': case 'command': case 'commandExecution': {
        const cmd = i.command || String(summary || '').replace(/\s*\[[^\]]*\]\s*$/, '');
        return { icon: 'terminal', verb: 'Ran', pending: 'Run', target: ui.firstLine(cmd, 80), full: i.description ? `${i.description}\n${cmd}` : cmd, code: true };
      }
      case 'apply_patch': case 'fileChange': return { icon: 'edit', verb: 'Edited', target: String(summary || '').replace(/\s*\[[^\]]*\]\s*$/, '').split(', ').map(ui.basename).join(', '), full: summary };
      case 'WebFetch': return { icon: 'globe', verb: 'Fetched', target: host(i.url), full: i.url };
      case 'WebSearch': case 'web_search': case 'webSearch': return { icon: 'globe', verb: 'Searched the web', target: i.query ? `“${i.query}”` : '', full: i.query };
      case 'TodoWrite': return { icon: 'checklist', verb: 'Updated the plan', target: Array.isArray(i.todos) ? `${i.todos.length} items` : '' };
      case 'Agent': case 'Task': case 'task': return { icon: 'hubot', verb: 'Delegated', target: i.description || ui.firstLine(i.prompt, 80), full: i.prompt };
      default:
        if (/^collab:spawn_agent|^spawn_agent/.test(n)) return { icon: 'hubot', verb: 'Delegated', target: ui.firstLine(summary, 80), full: summary };
        if (/^collab:/.test(n)) return { icon: 'watch', verb: n.replace('collab:', '').replace(/_/g, ' ').replace(/^\w/, c => c.toUpperCase()), target: '' };
        if (/mcp/i.test(n)) return { icon: 'plug', verb: 'Used', target: n, full: summary };
        return { icon: 'tools', verb: n, target: ui.firstLine(Object.values(i).find(v => typeof v === 'string') || '', 60), full: summary };
    }
  }

  class Conversation {
    /** opts: { post(msg), compact?: bool (grid tiles: fewer details) } */
    constructor(root, opts) {
      this.root = root; this.opts = opts || {};
      this.seen = new Set(); this.turns = []; this.tools = new Map(); this.perms = new Map(); this.children = new Map();
      this.childInfo = new Map(); this.rootId = undefined; this.attention = undefined; this.active = false;
      this.banner = el('div', 'conv-banner'); this.banner.hidden = true; this.banner.setAttribute('role', 'status');
      this.list = el('div', 'conv-turns');
      this.working = el('div', 'working'); this.working.hidden = true; this.working.setAttribute('aria-live', 'polite');
      this.working.append(el('span', 'working-dot'), el('span', 'working-label', 'Working…'));
      root.replaceChildren(this.banner, this.list, this.working);
    }

    setRun(msg) {
      this.rootId = msg.run.id;
      this.status = msg.run.status;
      this.active = ['queued', 'starting', 'running'].includes(msg.run.status);
      this.attention = msg.run.attention && msg.run.attention.kind === 'permission' ? msg.run.attention.request_id : undefined;
      for (const c of msg.children || []) this.childInfo.set(c.id, c);
      for (const [id, card] of this.perms) this.renderPermission(id, card);
      for (const [id, block] of this.children) this.renderChildHeader(id, block);
      this.updateWorking();
    }

    truncated(text) { this.banner.hidden = false; this.banner.replaceChildren(ui.icon('history', 'sm'), el('span', null, text)); }

    turn() {
      if (!this.turns.length) this.newTurn({ n: 0, prompt: '' }, true);
      return this.turns[this.turns.length - 1];
    }

    newTurn(t, implicit, ev) {
      const box = el('section', 'turn'); box.dataset.turn = t.n;
      box.setAttribute('aria-label', implicit ? 'Before the first turn' : `Turn ${t.n}`);
      const body = el('div', 'turn-body');
      const foot = el('div', 'turn-foot'); foot.hidden = true;
      const done = el('span', 'done'); const dur = el('span', 'dur'); const usage = el('span', 'usage');
      foot.append(done, dur, usage);
      if (t.prompt) {
        const p = el('div', 'msg user'); p.setAttribute('aria-label', 'You');
        const text = el('div', 'text', t.prompt);
        p.append(text); box.append(p);
      }
      box.append(body, foot);
      this.list.append(box);
      const turn = { n: t.n, box, body, foot, usage, done, dur, started: ev && (ev.ts_ms || ev.ts) };
      this.turns.push(turn);
      return turn;
    }

    /** Container for an event: the current turn for the root run, or the child's own block. */
    container(ev) {
      if (!ev.run_id || ev.run_id === this.rootId) return this.turn().body;
      return this.childBlock(ev.run_id).body;
    }

    /** The group of consecutive tool calls at the end of a container (created when needed). */
    steps(host) {
      const last = host.lastElementChild;
      if (last && last.classList.contains('steps')) return last._steps;
      const box = el('div', 'steps');
      const fold = el('details', 'steps-fold'); const sum = el('summary', 'steps-head'); const list = el('div', 'steps-list');
      fold.append(sum, list);
      const edits = el('div', 'steps-edits');
      box.append(list, edits);
      const g = { box, fold, sum, list, edits, count: 0, verbs: new Map(), failed: 0 };
      box._steps = g;
      host.append(box);
      return g;
    }

    /** Folds a group into one summary line once it has more than one call. */
    regroup(g) {
      if (g.count > 1 && g.list.parentElement !== g.fold) { g.box.insertBefore(g.fold, g.edits); g.fold.append(g.list); }
      if (g.count > 1) {
        const parts = [...g.verbs].map(([v, n]) => (n > 1 ? `${v} ${n}×` : v));
        g.sum.replaceChildren(ui.icon('tools', 'sm'), el('span', 'steps-label', `${g.count} steps`), el('span', 'steps-verbs', parts.join(' · ')));
        if (g.failed) g.sum.append(el('span', 'steps-failed', `${g.failed} failed`));
        g.sum.title = parts.join(', ');
      }
    }

    childBlock(runId, hint) {
      let block = this.children.get(runId);
      if (block) return block;
      const info = hint || this.childInfo.get(runId) || { id: runId, title: 'Sub-agent' };
      if (hint) this.childInfo.set(runId, { ...this.childInfo.get(runId), ...hint });
      const details = el('details', 'child'); details.open = !this.opts.compact; details.dataset.run = runId;
      const summary = el('summary', 'child-head');
      const body = el('div', 'child-body');
      details.append(summary, body);
      block = { el: details, summary, body };
      this.children.set(runId, block);
      this.renderChildHeader(runId, block);
      const parent = info.parent || info.parent_run_id;
      const scope = parent && parent !== this.rootId ? parent : this.rootId;
      const evidence = String(info.evidence || info.relation_source || '');
      let hostCard;
      for (const [key, card] of this.tools) {
        if (!key.startsWith(scope + '\u0000')) continue;
        if (card.id && (card.id === info.native_id || evidence.includes(card.id))) hostCard = card;
      }
      if (!hostCard) {
        for (const card of [...this.tools.values()].reverse()) {
          if (card.run === scope && SPAWN_TOOLS.test(card.name) && !card.kids.children.length) { hostCard = card; break; }
        }
      }
      if (hostCard) { hostCard.kids.append(details); block.nested = hostCard.id || true; }
      else if (scope !== this.rootId && this.children.has(scope)) this.children.get(scope).body.append(details);
      else this.turn().body.append(details);
      return block;
    }

    renderChildHeader(runId, block) {
      const info = this.childInfo.get(runId) || {};
      const st = info.status || 'unknown';
      const title = el('span', 'child-title', info.title || 'Sub-agent');
      block.summary.replaceChildren(ui.icon('type-hierarchy-sub', 'sm child-mark'), title, ui.status(st));
      block.summary.title = [info.title, ui.statusText(st), info.evidence || info.relation_source].filter(Boolean).join('\n');
    }

    toolCard(ev, id, name) {
      const key = (ev.run_id || this.rootId) + '\u0000' + (id || 'seq' + ev.seq);
      let card = this.tools.get(key);
      if (card) return card;
      const details = el('details', 'tool'); details.dataset.tool = id || '';
      const summary = el('summary', 'tool-head');
      const iconEl = el('span', 'tool-icon'); const verbEl = el('span', 'tool-verb'); const sumEl = el('span', 'tool-summary'); const statusEl = el('span', 'tool-result');
      summary.append(iconEl, verbEl, sumEl, statusEl);
      const input = el('div', 'tool-section'); const output = el('div', 'tool-section');
      const kids = el('div', 'tool-children');
      details.append(summary, input, output);
      card = { el: details, id, name: name || '', run: ev.run_id || this.rootId, iconEl, verbEl, sumEl, statusEl, input, output, kids, data: {} };
      details.addEventListener('toggle', () => { if (details.open) this.fillTool(card); });
      const hostEl = this.container(ev);
      if (SPAWN_TOOLS.test(card.name)) { hostEl.append(details, kids); }
      else {
        const g = this.steps(hostEl);
        g.list.append(details, kids); g.count++; card.group = g;
        this.regroup(g);
      }
      this.tools.set(key, card);
      if (id) {
        for (const [runId, block] of this.children) {
          const info = this.childInfo.get(runId) || {};
          const sameParent = !info.parent || info.parent === card.run || (card.run === this.rootId && info.parent === this.rootId);
          if (!block.nested && sameParent && (info.native_id === id || String(info.evidence || '').split(' inside ')[0].split(/[\s()]+/).includes(id))) { kids.append(block.el); block.nested = id; }
        }
      }
      this.label(card);
      return card;
    }

    label(card) {
      const d = describe(card.name, card.data.input, card.summary);
      card.desc = d;
      card.el.dataset.name = card.name;
      const done = card.status && !/running|started|inProgress|in_progress/.test(card.status);
      card.iconEl.replaceChildren(ui.icon(d.icon, 'sm'));
      card.verbEl.textContent = !done && d.pending && card.status !== undefined ? d.pending : d.verb;
      card.sumEl.textContent = d.target || '';
      card.sumEl.classList.toggle('mono', !!d.code);
      card.el.title = [d.full, card.name !== d.verb ? `(${card.name})` : ''].filter(Boolean).join(' ');
      if (card.group) {
        const g = card.group; g.verbs = new Map();
        for (const c of this.tools.values()) if (c.group === g) g.verbs.set(c.desc?.verb || c.name, (g.verbs.get(c.desc?.verb || c.name) || 0) + 1);
        this.regroup(g);
      }
      this.renderResult(card);
    }

    renderResult(card) {
      const st = card.data.status || card.status;
      const failed = card.data.is_error || /failed|error|declined/.test(st || '');
      const running = !st || /running|started|inProgress|in_progress/.test(st);
      const exit = /exit (\d+)/.exec(card.summary || '');
      const r = card.statusEl; r.className = 'tool-result'; r.replaceChildren();
      if (failed) { r.classList.add('bad'); r.append(ui.icon('error', 'xs'), el('span', null, exit && exit[1] !== '0' ? `exit ${exit[1]}` : 'failed')); }
      else if (running && !this.finishedTurn(card)) { r.classList.add('run'); r.append(el('span', 'mini-dot')); }
      else if (card.desc && (card.desc.added || card.desc.removed)) { r.append(el('span', 'add', `+${card.desc.added || 0}`), el('span', 'del', `−${card.desc.removed || 0}`)); }
      else r.append(ui.icon('check', 'xs ok'));
      if (card.group && failed !== !!card.failedCounted) { card.group.failed += failed ? 1 : -1; card.failedCounted = failed; this.regroup(card.group); }
    }

    finishedTurn(card) { return !this.active && card.run === this.rootId; }

    fillTool(card) {
      const d = card.data;
      card.input.replaceChildren(); card.output.replaceChildren();
      const input = d.input !== undefined && d.input !== null ? d.input : card.summary;
      if (input) {
        const parsed = parseInput(input);
        const text = parsed.command || parsed.content || parsed.new_string || (typeof input === 'string' ? input : JSON.stringify(input, null, 2));
        card.input.append(el('div', 'label', parsed.command ? 'Command' : parsed.content ? 'Content' : parsed.new_string ? 'New text' : 'Input'), el('pre', 'code', String(text).slice(0, 20000)));
      }
      if (d.output) card.output.append(el('div', 'label', d.is_error ? 'Error' : 'Result'), el('pre', 'code' + (d.is_error ? ' error' : ''), String(d.output).slice(0, 20000)));
      else card.output.append(el('div', 'muted small', d.status && d.status !== 'started' ? 'No output reported.' : 'Waiting for the result…'));
    }

    updateWorking(label) {
      if (label) this.working.querySelector('.working-label').textContent = label;
      this.working.hidden = !this.active;
      if (this.active) this.list.after(this.working);
    }

    add(ev) {
      if (this.seen.has(ev.seq)) return;
      this.seen.add(ev.seq);
      const p = ev.payload || {};
      const child = ev.run_id && ev.run_id !== this.rootId;
      switch (ev.kind) {
        case 'turn_started': if (!child) { this.stopping = false; this.newTurn(p.turn || { n: this.turns.length + 1, prompt: '' }, false, ev); this.active = true; this.updateWorking('Working…'); } break;
        case 'interrupt_requested': if (!child) this.stopping = true; break;
        case 'output': {
          const role = p.role || 'assistant';
          if (role === 'system') break; // session notes stay in the event log
          if (role === 'reasoning' || role === 'plan') {
            const d = el('details', 'thinking'); const s = el('summary');
            s.append(ui.icon(role === 'plan' ? 'checklist' : 'lightbulb', 'sm'), el('span', null, role === 'plan' ? 'Plan' : 'Thinking'));
            const t = el('div', 'text md'); window.OverseerMarkdown.render(t, p.text || '', this.opts);
            d.append(s, t); this.container(ev).append(d);
          } else {
            const m = el('div', 'msg ' + (role === 'assistant' ? 'agent' : role));
            m.setAttribute('aria-label', role === 'assistant' ? (child ? 'Sub-agent' : 'Agent') : role);
            const text = el('div', 'text md');
            if (role === 'assistant') window.OverseerMarkdown.render(text, p.text || '', this.opts);
            else text.textContent = p.text || '';
            m.append(text);
            this.container(ev).append(m);
            if (!child) this.updateWorking('Working…');
          }
          break;
        }
        case 'tool': {
          const card = this.toolCard(ev, p.id, p.name);
          if (p.name) card.name = p.name;
          card.summary = p.summary || '';
          const st = /\[(completed|failed|declined|inProgress|in_progress|running|error)[^\]]*\]\s*$/.exec(p.summary || '');
          card.status = st ? st[1] : card.status || 'running';
          this.label(card);
          if (card.el.open) this.fillTool(card);
          if (!child && card.desc) this.updateWorking(`${card.desc.pending || card.desc.verb} ${card.desc.target || ''}`.trim() + '…');
          break;
        }
        case 'tool_result': {
          const card = this.toolCard(ev, p.id, undefined);
          for (const k of ['input', 'output', 'status', 'is_error']) if (p[k] !== undefined && p[k] !== null) card.data[k] = p[k];
          card.status = p.is_error ? 'failed' : p.status || 'completed';
          this.label(card);
          if (card.el.open) this.fillTool(card);
          break;
        }
        case 'file_activity': {
          const hostEl = this.container(ev);
          const last = hostEl.lastElementChild;
          const target = last && last.classList.contains('steps') ? last._steps.edits : hostEl;
          const row = el('div', 'edit');
          row.append(ui.icon('diff', 'sm edit-icon'));
          for (const path of p.paths || []) {
            const b = el('button', 'link edit-path', ui.basename(path)); b.type = 'button';
            b.title = `${path}\nOpen at the edited hunk in the review${ev.confidence && ev.confidence !== 'reported' ? ` (${ev.confidence})` : ''}`;
            b.addEventListener('click', () => this.opts.post({ type: 'openEdit', runId: ev.run_id || this.rootId, path }));
            row.append(b);
          }
          target.append(row);
          break;
        }
        case 'permission': {
          const card = { el: el('div', 'perm-card'), tool: p.tool, input: p.input, run: ev.run_id || this.rootId };
          this.perms.set(p.request_id, card);
          this.container(ev).append(card.el);
          this.renderPermission(p.request_id, card);
          break;
        }
        case 'permission_answered': {
          const card = this.perms.get(p.request_id);
          if (card) { card.answer = p.allow ? 'allowed' : 'denied'; this.renderPermission(p.request_id, card); }
          break;
        }
        case 'error': {
          // Quiet endings (AC-78): a stop is not an error, and an error with nothing to say shows nothing.
          if (!child && (this.stopping || !String(p.message || '').trim())) break;
          if (!child) this.turn().hadError = true;
          const TITLES = { auth: 'Signed out', rate_limit: 'Rate limited', quota: 'Usage limit reached', network: 'Connection problem' };
          const e = el('div', 'error-block'); e.setAttribute('role', 'alert');
          const head = el('div', 'error-head'); head.append(ui.icon('error', 'sm'), el('strong', null, TITLES[p.class] || 'Error'));
          e.append(head, el('div', 'text', p.message || ''));
          e.title = p.class || 'error';
          if (p.class === 'auth') {
            const b = el('button', 'btn sm sign-in-again', 'Sign in again'); b.type = 'button';
            b.setAttribute('aria-label', 'Sign in again with this run\'s account');
            b.addEventListener('click', () => this.opts.post({ type: 'signIn' }));
            e.append(b);
          }
          this.container(ev).append(e);
          break;
        }
        case 'child': {
          const c = p.child || {};
          this.childBlock(c.id, { id: c.id, title: c.title, status: c.status, native_id: c.native_id, parent: c.parent_run_id, evidence: p.evidence || c.relation_source, confidence: c.relation_confidence });
          break;
        }
        case 'child_reparented': {
          const block = this.children.get(p.child_run_id);
          if (!block) break;
          const info = this.childInfo.get(p.child_run_id) || {};
          info.parent = p.parent_run_id; this.childInfo.set(p.child_run_id, info);
          const parent = p.parent_run_id === this.rootId ? null : this.childBlock(p.parent_run_id);
          if (parent && !parent.el.contains(block.el) && !block.el.contains(parent.el)) { parent.body.append(block.el); block.nested = 'reparented'; }
          break;
        }
        case 'turn_done': {
          if (child) break;
          const t = this.turn(); t.foot.hidden = false; t.ended = true;
          // A turn the user stopped reads "Stopped"; a failed one shows its reason once.
          const stopped = !p.ok && this.stopping;
          const label = p.ok ? 'Done' : stopped ? 'Stopped' : 'Failed';
          const reason = !p.ok && !stopped && !t.hadError && p.summary ? String(p.summary).split('\n')[0].slice(0, 160) : '';
          t.done.replaceChildren(ui.icon(p.ok ? 'check' : stopped ? 'circle-slash' : 'error', 'xs'), el('span', null, reason ? `${label}: ${reason}` : label));
          t.done.className = 'done ' + (p.ok ? 'ok' : stopped ? 'stopped' : 'fail');
          if (p.summary && !p.ok) t.done.title = p.summary;
          this.stopping = false;
          const end = ev.ts_ms || ev.ts;
          if (t.started && end && end > t.started) t.dur.textContent = ui.duration(end - t.started);
          this.active = false; this.updateWorking();
          for (const c of this.tools.values()) if (c.run === this.rootId) this.renderResult(c);
          break;
        }
        case 'retention': this.truncated('Older history was trimmed. Raw output keeps everything.'); break;
        // Lines the parser does not understand stay in the event log and raw output, not the chat.
        case 'raw_unparsed': break;
        default: break;
      }
      if (ev.kind === 'status') this.status_(ev, p, child);
      if (ev.kind === 'usage') this.usage_(ev, p, child);
      if (!QUIET.has(ev.kind) && !Conversation.KNOWN.has(ev.kind)) this.container(ev).append(el('div', 'sys', ev.kind.replace(/_/g, ' ')));
    }

    status_(ev, p, child) {
      if (child) {
        const info = this.childInfo.get(ev.run_id) || {}; info.status = p.status; this.childInfo.set(ev.run_id, info);
        const block = this.children.get(ev.run_id); if (block) this.renderChildHeader(ev.run_id, block);
        return;
      }
      if (['interrupted', 'failed', 'disconnected', 'unknown', 'completed'].includes(p.status)) { this.active = false; this.updateWorking(); }
      if (['running', 'starting'].includes(p.status)) { this.active = true; this.updateWorking(); }
      if (p.status === 'waiting_for_user') { this.active = false; this.updateWorking(); }
      if (['interrupted', 'failed', 'disconnected', 'unknown'].includes(p.status)) {
        const t = this.turns.length ? this.turn() : undefined;
        // The turn's footer already says how it ended; one line is enough.
        if (t && t.ended && ['interrupted', 'failed'].includes(p.status)) return;
        if (t && p.status === 'interrupted') { t.foot.hidden = false; t.ended = true; t.done.replaceChildren(ui.icon('circle-slash', 'xs'), el('span', null, 'Stopped')); t.done.className = 'done stopped'; this.stopping = false; return; }
        const line = el('div', `sys status-line status-${p.status}`);
        line.append(ui.icon(p.status === 'interrupted' ? 'circle-slash' : 'error', 'xs'), el('span', null, ui.statusText(p.status)));
        if (p.reason) line.title = p.reason;
        this.container(ev).append(line);
      }
    }

    usage_(ev, p, child) {
      if (child) return;
      const t = this.turn();
      const u = p.usage || p.total || p.tokens || p;
      const pick = (...keys) => keys.map(k => u && u[k]).find(v => typeof v === 'number');
      const input = pick('input_tokens', 'inputTokens', 'input'), output = pick('output_tokens', 'outputTokens', 'output');
      const cached = pick('cache_read_input_tokens', 'cached_input_tokens', 'cachedInputTokens');
      const cost = typeof p.total_cost_usd === 'number' ? p.total_cost_usd : typeof p.cost === 'number' ? p.cost : undefined;
      if (input === undefined && output === undefined && cost === undefined) { if (p.rate_limits) t.limits = p.rate_limits; return; }
      const parts = [];
      if (input !== undefined || output !== undefined) parts.push(`${ui.compact((input || 0) + (output || 0))} tokens`);
      if (cost) parts.push(`$${cost < 0.01 ? cost.toFixed(4) : cost.toFixed(2)}`);
      t.usage.textContent = parts.join(' · ');
      t.usage.title = [input !== undefined && `${input.toLocaleString()} in`, output !== undefined && `${output.toLocaleString()} out`, cached !== undefined && `${cached.toLocaleString()} cached`, cost !== undefined && `$${cost.toFixed(4)}`].filter(Boolean).join(' · ');
      t.foot.hidden = false;
    }

    renderPermission(id, card) {
      const pending = !card.answer && this.attention === id;
      card.el.className = 'perm-card' + (pending ? ' pending' : ' ' + (card.answer || 'asked'));
      const d = describe(card.tool, card.input);
      const what = `${d.pending || d.verb} ${d.target || ''}`.trim();
      const head = el('div', 'perm-head');
      head.append(ui.icon(pending ? 'shield' : card.answer === 'allowed' ? 'check' : card.answer === 'denied' ? 'circle-slash' : 'shield', 'sm'),
        el('span', null, pending ? `Allow ${what}?` : card.answer === 'allowed' ? `Allowed · ${what}` : card.answer === 'denied' ? `Denied · ${what}` : `Asked · ${what}`));
      head.title = d.full || card.tool;
      const kids = [head];
      if (pending) {
        const parsed = parseInput(card.input);
        const preview = parsed.command || parsed.content || parsed.new_string;
        if (preview) kids.push(el('pre', 'code perm-preview', String(preview).split('\n').slice(0, 8).join('\n')));
        const allow = el('button', 'btn primary sm', 'Allow once'); allow.type = 'button'; allow.dataset.permission = 'allow';
        allow.addEventListener('click', () => this.opts.post({ type: 'permission', request_id: id, allow: true }));
        const deny = el('button', 'btn sm', 'Deny'); deny.type = 'button'; deny.dataset.permission = 'deny';
        deny.addEventListener('click', () => this.opts.post({ type: 'permission', request_id: id, allow: false }));
        const row = el('div', 'perm-actions'); row.append(allow, deny); kids.push(row);
      }
      const det = el('details', 'perm-input'); const s = el('summary', null, 'Request'); s.title = 'The exact request the agent sent';
      det.append(s, el('pre', 'code', JSON.stringify(card.input, null, 2).slice(0, 4000)));
      if (!this.opts.compact) kids.push(det);
      card.el.replaceChildren(...kids);
    }
  }
  Conversation.KNOWN = new Set(['turn_started', 'output', 'tool', 'tool_result', 'file_activity', 'permission', 'permission_answered', 'error', 'child', 'child_reparented', 'turn_done', 'retention', 'raw_unparsed']);
  Conversation.describe = describe;

  window.OverseerConversation = Conversation;
})();
