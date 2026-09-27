// Agent grid (AC-58): agents tiled like terminal panes (1, 2, 2×2, 3×2 or 3×3 by count, up to a
// configurable maximum). Each tile streams its agent's conversation compactly, with inline
// Allow / Deny and a one-line reply; Enter or a click on the title opens the agent in the chat;
// tiles can be pinned; arrow keys move between tiles.
(function () {
  const ui = window.OverseerUI, el = ui.el;
  const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user']);
  const SHAPES = { 1: [1, 1], 2: [2, 1], 3: [2, 2], 4: [2, 2], 5: [3, 2], 6: [3, 2], 7: [3, 3], 8: [3, 3], 9: [3, 3] };

  function create(host, { post, open, getState }) {
    const tiles = new Map(); // runId -> tile
    let visible = false, order = [];
    const board = el('div', 'grid'); board.setAttribute('role', 'grid'); board.setAttribute('aria-label', 'Agents');
    const empty = el('div', 'empty-state'); empty.hidden = true;
    empty.append(ui.icon('layout'), el('div', null, 'No agents running'));
    const startBtn = el('button', 'btn primary sm', 'New agent'); startBtn.type = 'button'; startBtn.dataset.action = 'new-agent-grid';
    startBtn.addEventListener('click', () => post({ type: 'focusComposer' }));
    empty.append(startBtn);
    host.append(board, empty);

    function pick(state) {
      const pinned = new Set(state.pinned || []);
      const roots = state.runs.filter(r => !r.parent_run_id && (ACTIVE.has(r.status) || pinned.has(r.id)));
      const rank = r => (r.status === 'waiting_for_user' ? 0 : ACTIVE.has(r.status) ? 1 : 2);
      roots.sort((a, b) => rank(a) - rank(b) || (b.ended_ms || b.created_ms) - (a.ended_ms || a.created_ms));
      return roots.slice(0, Math.max(1, Math.min(9, state.gridMax || 6)));
    }

    function makeTile(run) {
      const t = el('article', 'tile'); t.tabIndex = 0; t.dataset.run = run.id; t.setAttribute('role', 'gridcell');
      const head = el('header', 'tile-head');
      const status = el('span', 'tile-status');
      const title = el('button', 'tile-title'); title.type = 'button';
      const who = el('span', 'tile-who');
      const pin = ui.iconButton('pin', 'Pin to grid', { cls: 'sm tile-pin', pressed: false });
      const openBtn = ui.iconButton('screen-full', 'Open agent', { cls: 'sm tile-open', shortcut: 'Enter' });
      head.append(status, title, who, pin, openBtn);
      const bodyEl = el('div', 'tile-body');
      const convEl = el('div', 'tile-conv'); bodyEl.append(convEl);
      const perm = el('div', 'tile-perm'); perm.hidden = true;
      const foot = el('form', 'tile-input');
      const input = el('input'); input.placeholder = 'Reply'; input.setAttribute('aria-label', `Reply to ${run.title}`);
      const send = ui.iconButton('arrow-up', 'Send', { cls: 'sm' }); send.type = 'submit';
      foot.append(input, send);
      t.append(head, bodyEl, perm, foot);
      const tile = { el: t, run, status, title, who, pin, openBtn, bodyEl, convEl, perm, input, send, conv: new window.OverseerConversation(convEl, { post: m => post({ ...m, runId: m.runId || run.id, scope: 'tile' }), compact: true }) };
      title.addEventListener('click', () => open(run.id));
      openBtn.addEventListener('click', () => open(run.id));
      pin.addEventListener('click', () => post({ type: 'pin', runId: run.id, on: pin.getAttribute('aria-pressed') !== 'true' }));
      foot.addEventListener('submit', e => { e.preventDefault(); const text = input.value.trim(); if (!text || input.disabled) return; post({ type: 'followUp', runId: run.id, text, scope: 'tile' }); input.value = ''; });
      t.addEventListener('keydown', e => {
        if (e.target !== t) return;
        if (e.key === 'Enter') { open(run.id); e.preventDefault(); }
        else if (/^Arrow/.test(e.key)) { move(run.id, e.key); e.preventDefault(); }
      });
      return tile;
    }

    function move(runId, key) {
      const i = order.indexOf(runId); if (i < 0) return;
      const cols = Number(board.style.getPropertyValue('--cols')) || 1;
      const j = key === 'ArrowRight' ? i + 1 : key === 'ArrowLeft' ? i - 1 : key === 'ArrowDown' ? i + cols : i - cols;
      const next = tiles.get(order[Math.max(0, Math.min(order.length - 1, j))]); next && next.el.focus();
    }

    function update(tile, run, state) {
      tile.run = run;
      tile.status.replaceChildren(ui.status(run.status, run.attention?.kind));
      tile.title.textContent = run.title; tile.title.title = `Open ${run.title}`;
      const p = run.profile_id && state.profiles.find(x => x.id === run.profile_id);
      tile.who.replaceChildren(ui.harnessMark(run.harness, 12)); tile.who.title = [ui.HARNESS[run.harness] || run.harness, p?.name, run.model].filter(Boolean).join(' · ');
      const pinned = (state.pinned || []).includes(run.id);
      tile.pin.setAttribute('aria-pressed', String(pinned)); tile.pin.title = pinned ? 'Unpin' : 'Pin to grid'; tile.pin.setAttribute('aria-label', tile.pin.title);
      tile.el.classList.toggle('needs', run.status === 'waiting_for_user');
      const att = run.attention && run.attention.kind === 'permission' ? run.attention : undefined;
      tile.perm.hidden = !att;
      if (att) {
        const d = window.OverseerConversation.describe(att.tool, att.input);
        const text = el('span', 'tile-perm-text', `Allow ${d.pending || d.verb} ${d.target || ''}?`); text.title = d.full || att.tool;
        const allow = el('button', 'btn primary sm', 'Allow'); allow.type = 'button'; allow.dataset.permission = 'allow';
        allow.addEventListener('click', () => post({ type: 'permission', runId: run.id, request_id: att.request_id, allow: true, scope: 'tile' }));
        const deny = el('button', 'btn sm', 'Deny'); deny.type = 'button'; deny.dataset.permission = 'deny';
        deny.addEventListener('click', () => post({ type: 'permission', runId: run.id, request_id: att.request_id, allow: false, scope: 'tile' }));
        tile.perm.replaceChildren(ui.icon('shield', 'sm'), text, allow, deny);
      }
      const busy = ACTIVE.has(run.status) && run.harness !== 'generic';
      tile.input.disabled = busy || String(run.capabilities?.follow_up || '').startsWith('unsupported');
      tile.input.placeholder = busy ? 'Working…' : 'Reply';
    }

    function layout(state) {
      const runs = pick(state);
      order = runs.map(r => r.id);
      for (const [id, tile] of tiles) if (!order.includes(id)) { tile.el.remove(); tiles.delete(id); }
      const [cols, rows] = SHAPES[runs.length] || [1, 1];
      board.style.setProperty('--cols', cols); board.style.setProperty('--rows', rows);
      board.dataset.count = String(runs.length);
      for (const run of runs) {
        let tile = tiles.get(run.id);
        if (!tile) { tile = makeTile(run); tiles.set(run.id, tile); }
        update(tile, run, state);
        board.append(tile.el);
      }
      empty.hidden = runs.length > 0; board.hidden = runs.length === 0;
      if (visible) post({ type: 'gridSubscribe', runIds: order });
    }

    return {
      open() { visible = true; layout(getState()); setTimeout(() => (tiles.get(order[0])?.el || startBtn).focus(), 0); },
      close() { if (!visible) return; visible = false; post({ type: 'gridSubscribe', runIds: [] }); },
      onState(state) { if (visible) layout(state); },
      run(m) { const t = tiles.get(m.run.id); if (t) t.conv.setRun(m); },
      history(m) { const t = tiles.get(m.root); if (!t) return; for (const x of m.events) t.conv.add(x.event); t.bodyEl.scrollTop = t.bodyEl.scrollHeight; },
      events(items) {
        const touched = new Set();
        for (const x of items) { const t = tiles.get(x.root); if (!t) continue; t.conv.add(x.event); touched.add(t); }
        for (const t of touched) { t.bodyEl.scrollTop = t.bodyEl.scrollHeight; t.el.dataset.updated = String(Date.now()); }
      },
    };
  }
  window.OverseerGrid = { create };
})();
