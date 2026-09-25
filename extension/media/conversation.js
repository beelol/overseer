// Conversation renderer for Overseer run views (webview side, no dependencies).
// Turns normalized daemon events into a conversation: prompts and agent messages as turns,
// collapsible tool calls with inputs/results, file edits that open the review at the hunk,
// inline permission requests with their decision, native children nested under the tool
// call that spawned them, highlighted errors and per-turn usage. Used by the run panel and
// the Overseer view. Everything is built with textContent (never innerHTML) for safety.
(function () {
  const SPAWN_TOOLS = /^(Agent|Task|task|collab:spawn_agent)$/;
  const QUIET = new Set(['session', 'task_created', 'reattached', 'interrupt_requested', 'workspace_removed', 'background_notice', 'daemon_stopping']);

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }
  function fmtTokens(p) {
    const u = p.usage || p.total || p.tokens || p;
    const pick = (...keys) => keys.map(k => u && u[k]).find(v => typeof v === 'number');
    const input = pick('input_tokens', 'inputTokens', 'input'), output = pick('output_tokens', 'outputTokens', 'output');
    const parts = [];
    if (input !== undefined) parts.push(`${input.toLocaleString()} in`);
    if (output !== undefined) parts.push(`${output.toLocaleString()} out`);
    const cost = typeof p.total_cost_usd === 'number' ? p.total_cost_usd : typeof p.cost === 'number' ? p.cost : undefined;
    if (cost !== undefined) parts.push(`$${cost.toFixed(4)}`);
    if (p.rate_limits) parts.push('rate limits reported');
    return parts.join(' · ') || JSON.stringify(p).slice(0, 160);
  }

  class Conversation {
    /** opts: { post(msg), label(runId) } */
    constructor(root, opts) {
      this.root = root; this.opts = opts;
      this.seen = new Set(); this.turns = []; this.tools = new Map(); this.perms = new Map(); this.children = new Map();
      this.childInfo = new Map(); this.rootId = undefined; this.attention = undefined;
      this.banner = el('div', 'conv-banner'); this.banner.hidden = true; this.banner.setAttribute('role', 'status');
      this.list = el('div', 'conv-turns');
      root.replaceChildren(this.banner, this.list);
    }

    setRun(msg) {
      this.rootId = msg.run.id;
      this.attention = msg.run.attention && msg.run.attention.kind === 'permission' ? msg.run.attention.request_id : undefined;
      for (const c of msg.children || []) this.childInfo.set(c.id, c);
      for (const [id, card] of this.perms) this.renderPermission(id, card);
      for (const [id, block] of this.children) this.renderChildHeader(id, block);
    }

    truncated(text) { this.banner.hidden = false; this.banner.textContent = text; }

    turn() {
      if (!this.turns.length) this.newTurn({ n: 0, prompt: '' }, true);
      return this.turns[this.turns.length - 1];
    }

    newTurn(t, implicit) {
      const box = el('section', 'turn'); box.dataset.turn = t.n;
      const head = el('div', 'turn-head');
      head.append(el('span', 'turn-n', implicit ? 'Before the first turn' : `Turn ${t.n}`));
      const body = el('div', 'turn-body');
      const foot = el('div', 'turn-foot'); foot.hidden = true;
      const usage = el('span', 'usage'); const done = el('span', 'done');
      foot.append(done, usage);
      box.append(head);
      if (t.prompt) { const p = el('div', 'msg user'); p.append(el('div', 'who', 'You'), el('div', 'text', t.prompt)); box.append(p); }
      box.append(body, foot);
      this.list.append(box);
      const turn = { n: t.n, box, body, foot, usage, done };
      this.turns.push(turn);
      return turn;
    }

    /** Container for an event: the current turn for the root run, or the child's own block. */
    container(ev) {
      if (!ev.run_id || ev.run_id === this.rootId) return this.turn().body;
      return this.childBlock(ev.run_id).body;
    }

    childBlock(runId, hint) {
      let block = this.children.get(runId);
      if (block) return block;
      const info = hint || this.childInfo.get(runId) || { id: runId, title: 'native child' };
      if (hint) this.childInfo.set(runId, { ...this.childInfo.get(runId), ...hint });
      const details = el('details', 'child'); details.open = true; details.dataset.run = runId;
      const summary = el('summary', 'child-head');
      const body = el('div', 'child-body');
      details.append(summary, body);
      block = { el: details, summary, body };
      this.children.set(runId, block);
      this.renderChildHeader(runId, block);
      // Nest under the tool call that spawned it (same native id, or named in the evidence).
      const parent = info.parent || info.parent_run_id;
      const scope = parent && parent !== this.rootId ? parent : this.rootId;
      const evidence = String(info.evidence || info.relation_source || '');
      let host;
      for (const [key, card] of this.tools) {
        if (!key.startsWith(scope + '\u0000')) continue;
        if (card.id && (card.id === info.native_id || evidence.includes(card.id))) host = card;
      }
      if (!host) {
        for (const card of [...this.tools.values()].reverse()) {
          if (card.run === scope && SPAWN_TOOLS.test(card.name) && !card.kids.children.length) { host = card; break; }
        }
      }
      if (host) { host.kids.append(details); block.nested = host.id || true; }
      else if (scope !== this.rootId && this.children.has(scope)) this.children.get(scope).body.append(details);
      else this.turn().body.append(details);
      return block;
    }

    renderChildHeader(runId, block) {
      const info = this.childInfo.get(runId) || {};
      block.summary.replaceChildren(el('span', 'child-mark', '↳'), el('span', 'child-title', info.title || 'native child'),
        el('span', `badge status-${info.status || 'unknown'}`, (info.status || 'unknown').replace(/_/g, ' ')));
      block.summary.title = [info.evidence || info.relation_source, info.confidence || info.relation_confidence].filter(Boolean).join('\n');
    }

    toolCard(ev, id, name) {
      const key = (ev.run_id || this.rootId) + '\u0000' + (id || 'seq' + ev.seq);
      let card = this.tools.get(key);
      if (card) return card;
      const details = el('details', 'tool'); details.dataset.tool = id || '';
      const summary = el('summary', 'tool-head');
      const nameEl = el('span', 'tool-name', name || 'tool'); const sumEl = el('span', 'tool-summary'); const statusEl = el('span', 'badge');
      summary.append(el('span', 'tool-icon', '⚙'), nameEl, sumEl, statusEl);
      const input = el('div', 'tool-section'); const output = el('div', 'tool-section');
      const kids = el('div', 'tool-children');
      details.append(summary, input, output);
      card = { el: details, id, name: name || '', run: ev.run_id || this.rootId, nameEl, sumEl, statusEl, input, output, kids, data: {} };
      details.addEventListener('toggle', () => { if (details.open) this.fillTool(card); });
      const host = this.container(ev);
      host.append(details, kids);
      this.tools.set(key, card);
      // Some harnesses (Claude) report the child before the tool call that spawned it: adopt it.
      if (id) {
        for (const [runId, block] of this.children) {
          const info = this.childInfo.get(runId) || {};
          const sameParent = !info.parent || info.parent === card.run || (card.run === this.rootId && info.parent === this.rootId);
          if (!block.nested && sameParent && (info.native_id === id || String(info.evidence || '').split(' inside ')[0].split(/[\s()]+/).includes(id))) { kids.append(block.el); block.nested = id; }
        }
      }
      return card;
    }

    fillTool(card) {
      const d = card.data;
      card.input.replaceChildren(); card.output.replaceChildren();
      if (d.input !== undefined && d.input !== null) { card.input.append(el('div', 'label', 'Input'), el('pre', 'code', typeof d.input === 'string' ? d.input : JSON.stringify(d.input, null, 2))); }
      else if (card.sumEl.textContent) card.input.append(el('div', 'label', 'Input'), el('pre', 'code', card.sumEl.textContent));
      if (d.output) card.output.append(el('div', 'label', d.is_error ? 'Result (error)' : 'Result'), el('pre', 'code' + (d.is_error ? ' error' : ''), d.output));
      else card.output.append(el('div', 'muted', d.status && d.status !== 'started' ? `No output reported (${d.status}).` : 'Waiting for the result…'));
    }

    add(ev) {
      if (this.seen.has(ev.seq)) return;
      this.seen.add(ev.seq);
      const p = ev.payload || {};
      const child = ev.run_id && ev.run_id !== this.rootId;
      switch (ev.kind) {
        case 'turn_started': if (!child) this.newTurn(p.turn || { n: this.turns.length + 1, prompt: '' }); break;
        case 'output': {
          const role = p.role || 'assistant';
          if (role === 'reasoning' || role === 'plan') {
            const d = el('details', 'thinking'); d.append(el('summary', null, role === 'plan' ? 'Plan' : 'Reasoning'), el('div', 'text', p.text || ''));
            this.container(ev).append(d);
          } else {
            const m = el('div', 'msg ' + (role === 'assistant' ? 'agent' : role));
            m.append(el('div', 'who', role === 'assistant' ? (child ? 'Child agent' : 'Agent') : role), el('div', 'text', p.text || ''));
            this.container(ev).append(m);
          }
          break;
        }
        case 'tool': {
          const card = this.toolCard(ev, p.id, p.name);
          card.nameEl.textContent = p.name || card.name; card.name = p.name || card.name;
          card.sumEl.textContent = p.summary || '';
          const st = /\[(completed|failed|declined|inProgress|in_progress|running|error)[^\]]*\]\s*$/.exec(p.summary || '');
          if (st) this.toolStatus(card, st[1]);
          if (card.el.open) this.fillTool(card);
          break;
        }
        case 'tool_result': {
          const card = this.toolCard(ev, p.id, undefined);
          for (const k of ['input', 'output', 'status', 'is_error']) if (p[k] !== undefined && p[k] !== null) card.data[k] = p[k];
          if (p.status) this.toolStatus(card, p.is_error ? 'failed' : p.status);
          if (card.el.open) this.fillTool(card);
          break;
        }
        case 'file_activity': {
          const row = el('div', 'edit');
          row.append(el('span', 'edit-icon', '✎'), el('span', 'muted', p.kind ? `${p.kind}: ` : 'edited: '));
          for (const path of p.paths || []) {
            const b = el('button', 'link edit-path', path); b.title = `Open ${path} at the edited hunk in the review (${ev.confidence})`;
            b.addEventListener('click', () => this.opts.post({ type: 'openEdit', runId: ev.run_id || this.rootId, path }));
            row.append(b);
          }
          row.append(el('span', 'muted conf', ev.confidence === 'reported' ? '' : ` (${ev.confidence})`));
          this.container(ev).append(row);
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
          const e = el('div', 'error-block'); e.setAttribute('role', 'alert');
          e.append(el('strong', null, `✖ ${p.class || 'error'}`), el('div', 'text', p.message || ''));
          if (p.class === 'auth') {
            // Expired or missing login: reauthenticate this run's account through its own flow.
            const b = el('button', 'secondary sign-in-again', 'Sign in again');
            b.setAttribute('aria-label', 'Sign in again with this run\'s account');
            b.addEventListener('click', () => this.opts.post({ type: 'signIn' }));
            e.append(b);
          }
          this.container(ev).append(e);
          break;
        }
        case 'status': {
          if (child) {
            const info = this.childInfo.get(ev.run_id) || {}; info.status = p.status; this.childInfo.set(ev.run_id, info);
            const block = this.children.get(ev.run_id); if (block) this.renderChildHeader(ev.run_id, block);
          } else if (['interrupted', 'failed', 'disconnected', 'unknown'].includes(p.status)) {
            this.container(ev).append(el('div', `sys status-line status-${p.status}`, `${p.status.replace(/_/g, ' ')}${p.reason ? ': ' + p.reason : ''}`));
          }
          break;
        }
        case 'child': {
          const c = p.child || {};
          this.childBlock(c.id, { id: c.id, title: c.title, status: c.status, native_id: c.native_id, parent: c.parent_run_id, evidence: p.evidence || c.relation_source, confidence: c.relation_confidence });
          break;
        }
        case 'child_reparented': {
          // A delayed parent was reported later: move the child's block under its real parent.
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
          const t = this.turn(); t.foot.hidden = false;
          t.done.textContent = p.ok ? '■ Turn completed' : '■ Turn failed';
          t.done.className = 'done ' + (p.ok ? 'ok' : 'fail');
          if (p.summary && !p.ok) t.done.textContent += ': ' + p.summary;
          break;
        }
        case 'usage': {
          if (child) break;
          const t = this.turn(); t.foot.hidden = false;
          const onlyLimits = p.rate_limits && Object.keys(p).length === 1;
          if (onlyLimits) { t.limits = true; if (!t.tokens) t.usage.textContent = '∑ rate limits reported'; }
          else { t.tokens = fmtTokens(p); t.usage.textContent = '∑ ' + t.tokens + (t.limits ? ' · rate limits reported' : ''); t.usage.title = JSON.stringify(p); }
          break;
        }
        case 'retention': this.truncated(`Older history was truncated by the retention bound (${JSON.stringify(p)}). The raw output keeps the full stream.`); break;
        case 'raw_unparsed': {
          const d = el('details', 'thinking unparsed'); d.append(el('summary', null, `Unparsed line (${p.parser_version || 'parser'})`), el('pre', 'code', p.text || ''));
          this.container(ev).append(d);
          break;
        }
        default:
          if (!QUIET.has(ev.kind)) this.container(ev).append(el('div', 'sys', `${ev.kind}`));
      }
    }

    toolStatus(card, status) {
      const norm = /^(inProgress|in_progress|running|started)$/.test(status) ? 'running' : status === 'error' ? 'failed' : status;
      card.statusEl.className = `badge status-${norm}`; card.statusEl.textContent = norm.replace(/_/g, ' ');
      card.data.status = card.data.status || status;
    }

    renderPermission(id, card) {
      const pending = !card.answer && this.attention === id;
      card.el.className = 'perm-card' + (pending ? ' pending' : '');
      const head = el('div', 'perm-head', `${pending ? '⚠ Waiting for your permission' : card.answer === 'allowed' ? '✔ Allowed' : card.answer === 'denied' ? '✖ Denied' : '⚠ Permission requested'}: ${card.tool}`);
      const kids = [head];
      if (pending) {
        const allow = el('button', 'primary', 'Allow once'); allow.addEventListener('click', () => this.opts.post({ type: 'permission', request_id: id, allow: true }));
        const deny = el('button', 'secondary', 'Deny'); deny.addEventListener('click', () => this.opts.post({ type: 'permission', request_id: id, allow: false }));
        const row = el('div', 'perm-actions'); row.append(allow, deny); kids.push(row);
      }
      const d = el('details', 'perm-input'); d.append(el('summary', null, 'Request'), el('pre', 'code', JSON.stringify(card.input, null, 2).slice(0, 4000)));
      kids.push(d);
      card.el.replaceChildren(...kids);
    }
  }

  window.OverseerConversation = Conversation;
})();
