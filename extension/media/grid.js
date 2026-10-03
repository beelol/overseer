// Agent grid (AC-58, AC-104): agents tiled like terminal panes. The layout is a tree of splits the
// user builds by dragging: a tile (or an agent dropped from the side bar, AC-104) goes to the top,
// bottom, left or right edge of any tile or of the whole grid, with the drop previewed during the
// drag; tiles move the same way, and Alt+arrow moves the focused tile without a mouse. The grid holds
// at most 16 tiles (MAX); running and pinned agents fill free space by splitting the largest tile,
// up to the configured maximum. Each tile streams its agent's conversation compactly, with inline
// Allow / Deny and a one-line reply; Enter or a click on the title opens the agent in the chat;
// tiles can be pinned; arrow keys move between tiles. The layout is kept per window.
(function () {
  const ui = window.OverseerUI, el = ui.el;
  const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user', 'waiting_for_connection', 'waiting_for_memory']);
  const MAX = 16;

  // ---- Layout tree: { run } | { dir: 'row' | 'col', parts: [two or more nodes] } ----
  // Until the user arranges tiles, the tree is rebuilt from balanced shapes (1, 2, 2×2, 3×2, 3×3,
  // 4×3, 4×4 by count, as AC-58 had); once they drag or move a tile it is theirs (`custom`).
  const SHAPES = n => (n <= 1 ? [1, 1] : n === 2 ? [2, 1] : n <= 4 ? [2, 2] : n <= 6 ? [3, 2] : n <= 9 ? [3, 3] : n <= 12 ? [4, 3] : [4, 4]);
  const leaves = n => (!n ? [] : n.run ? [n.run] : n.parts.flatMap(leaves));
  const node = (dir, parts) => (parts.length === 1 ? parts[0] : { dir, parts: parts.flatMap(p => (p.dir === dir ? p.parts : [p])) });
  const balanced = ids => {
    const [cols] = SHAPES(ids.length);
    const rows = [];
    for (let i = 0; i < ids.length; i += cols) rows.push(node('row', ids.slice(i, i + cols).map(run => ({ run }))));
    return ids.length ? node('col', rows) : null;
  };
  const without = (n, id) => {
    if (!n) return null;
    if (n.run) return n.run === id ? null : n;
    const parts = n.parts.map(p => without(p, id)).filter(Boolean);
    return parts.length ? node(n.dir, parts) : null;
  };
  const dirOf = edge => (edge === 'left' || edge === 'right' ? 'row' : 'col');
  const before = edge => edge === 'left' || edge === 'top';
  /** Places `id` at `edge` of the tile `target` (or of the whole grid when target is null). */
  const insert = (n, target, edge, id) => {
    const add = { run: id }, dir = dirOf(edge);
    if (!n) return add;
    if (!target) return node(dir, before(edge) ? [add, n] : [n, add]);
    if (n.run) return n.run === target ? node(dir, before(edge) ? [add, n] : [n, add]) : n;
    // Beside a tile inside a split of the same direction: join that split as another equal part.
    const i = n.parts.findIndex(p => p.run === target);
    if (i >= 0 && n.dir === dir) { const parts = [...n.parts]; parts.splice(before(edge) ? i : i + 1, 0, add); return { dir, parts }; }
    return node(n.dir, n.parts.map(p => insert(p, target, edge, id)));
  };
  /** The largest tile, split across its longer side (w × h in board units). */
  const autoPlace = (n, id, w, h) => {
    if (!n) return { run: id };
    let best;
    const walk = (x, bw, bh) => {
      if (x.run) { if (!best || bw * bh > best.area + 1e-9) best = { run: x.run, area: bw * bh, edge: bw >= bh ? 'right' : 'bottom' }; return; }
      const k = x.parts.length;
      x.parts.forEach(p => walk(p, x.dir === 'row' ? bw / k : bw, x.dir === 'col' ? bh / k : bh));
    };
    walk(n, w, h);
    return insert(n, best.run, best.edge, id);
  };
  const valid = n => !n || (n.run ? typeof n.run === 'string' : (n.dir === 'row' || n.dir === 'col') && Array.isArray(n.parts) && n.parts.length >= 2 && n.parts.every(valid));

  function create(host, { post, open, getState, loadLayout, saveLayout }) {
    const tiles = new Map(); // runId -> tile
    let voiced = new Set(); // agents an open spoken request is for (Voice Mode, AC-169)
    const markVoice = t => { t.voice.hidden = !voiced.has(t.run.id); };
    let visible = false, order = [];
    const savedLayout = loadLayout?.();
    let tree = savedLayout && savedLayout.tree && valid(savedLayout.tree) ? savedLayout.tree : null, custom = !!savedLayout?.custom && !!tree;
    const pinning = new Set(); // placed agents whose pin has not come back from the host yet
    const board = el('div', 'grid'); board.setAttribute('role', 'grid'); board.setAttribute('aria-label', 'Agents');
    // Drag feedback: where the tile will go, or why it cannot.
    const preview = el('div', 'grid-drop'); preview.hidden = true; preview.setAttribute('aria-hidden', 'true');
    const full = el('div', 'grid-full', `The grid is full (${MAX})`); full.hidden = true; full.setAttribute('role', 'status');
    // AC-105: the tracked agent (its review is open beside the grid) and the way back to the grid alone.
    let tracked;
    const trackBar = el('div', 'grid-track'); trackBar.hidden = true; trackBar.setAttribute('role', 'status');
    const trackText = el('span', 'grid-track-text');
    const trackClose = el('button', 'btn sm', 'Grid alone'); trackClose.type = 'button'; trackClose.id = 'grid-untrack'; trackClose.title = 'Close the review and show the grid alone (Escape)';
    trackClose.addEventListener('click', () => post({ type: 'untrack' }));
    trackBar.append(ui.icon('eye', 'sm'), trackText, trackClose);
    const empty = el('div', 'empty-state'); empty.hidden = true;
    empty.append(ui.icon('layout'), el('div', null, 'No agents running'));
    const startBtn = el('button', 'btn primary sm', 'New agent'); startBtn.type = 'button'; startBtn.dataset.action = 'new-agent-grid';
    startBtn.addEventListener('click', () => post({ type: 'focusComposer' }));
    empty.append(startBtn);
    // AC-106: the grid names itself in its own header, with the map of open views and the reset.
    const head = el('header', 'grid-head');
    const whereBtn = ui.iconButton('location', 'Where am I', { cls: 'sm', shortcut: '⌥⌘M' }); whereBtn.id = 'grid-where';
    whereBtn.addEventListener('click', () => post({ type: 'command', command: 'overseer.whereAmI' }));
    const resetBtn = ui.iconButton('discard', 'Reset grid layout', { cls: 'sm' }); resetBtn.id = 'grid-reset';
    resetBtn.addEventListener('click', () => post({ type: 'command', command: 'overseer.resetGridLayout' }));
    // The name and count are the header's label and tooltip: the grid's visible text stays within
    // the AC-54 budget (the tiles already name every agent).
    head.setAttribute('role', 'toolbar'); head.setAttribute('aria-label', 'Agent grid');
    // The rollup by state (AC-255): the same counts as the side bar's summary row, whatever the tiles show.
    const rollupEl = el('span', 'grid-rollup'); rollupEl.id = 'grid-rollup'; rollupEl.setAttribute('role', 'status');
    head.append(ui.icon('layout', 'sm'), rollupEl, el('span', 'spacer'), whereBtn, resetBtn);
    function renderRollup(state) {
      const parts = window.OverseerRollup && state.rollup ? window.OverseerRollup.parts(state.rollup) : [];
      rollupEl.replaceChildren(...parts.map((p, i) => { const e = el('span', `grid-rollup-part rollup-${p.key}`, p.text); e.dataset.key = p.key; e.dataset.n = String(p.n); return i ? [el('span', 'grid-rollup-dot', '·'), e] : [e]; }).flat());
      rollupEl.setAttribute('aria-label', parts.length ? `Agents: ${parts.map(p => p.text).join(', ')}` : 'No agents');
      rollupEl.dataset.text = parts.map(p => p.text).join(' · ');
    }
    host.append(head, board, preview, full, trackBar, empty);

    /** Pinned (placed) agents always, up to 16; running agents fill up to the configured maximum. */
    function pick(state) {
      const pinned = new Set(state.pinned || []);
      const roots = state.runs.filter(r => !r.parent_run_id);
      const recent = (a, b) => (b.ended_ms || b.created_ms) - (a.ended_ms || a.created_ms);
      const kept = roots.filter(r => pinned.has(r.id)).sort(recent);
      const rank = r => (r.status === 'waiting_for_user' ? 0 : 1);
      const running = roots.filter(r => !pinned.has(r.id) && (ACTIVE.has(r.status) || r.queue?.paused && r.queue?.messages?.length)).sort((a, b) => rank(a) - rank(b) || recent(a, b));
      const room = Math.max(0, Math.max(1, Math.min(MAX, state.gridMax || 6)) - kept.length);
      return [...kept, ...running.slice(0, room)].slice(0, MAX);
    }

    function makeTile(run) {
      const t = el('article', 'tile'); t.tabIndex = 0; t.dataset.run = run.id; t.setAttribute('role', 'gridcell');
      const head = el('header', 'tile-head');
      const status = el('span', 'tile-status');
      const title = el('button', 'tile-title'); title.type = 'button';
      const who = el('span', 'tile-who');
      const voice = ui.icon('mic', 'xs'); voice.classList.add('tile-voice'); voice.title = 'A spoken request is for this agent'; voice.removeAttribute('aria-hidden'); voice.setAttribute('role', 'img'); voice.setAttribute('aria-label', 'Spoken request'); voice.hidden = true;
      const pin = ui.iconButton('pin', 'Pin to grid', { cls: 'sm tile-pin', pressed: false });
      const openBtn = ui.iconButton('screen-full', 'Open agent', { cls: 'sm tile-open', shortcut: 'Enter' });
      head.append(status, title, voice, who, pin, openBtn);
      head.draggable = true; head.title = 'Drag to move this tile';
      head.addEventListener('dragstart', e => { e.dataTransfer.setData('application/x-overseer-run', run.id); e.dataTransfer.setData('text/plain', run.title); e.dataTransfer.effectAllowed = 'move'; dragging = run.id; t.classList.add('dragging'); });
      head.addEventListener('dragend', () => { dragging = undefined; t.classList.remove('dragging'); hideDrop(); });
      const bodyEl = el('div', 'tile-body');
      const convEl = el('div', 'tile-conv'); bodyEl.append(convEl);
      const perm = el('div', 'tile-perm'); perm.hidden = true;
      const foot = el('form', 'tile-input');
      const input = el('input'); input.placeholder = 'Reply'; input.setAttribute('aria-label', `Reply to ${run.title}`);
      const send = ui.iconButton('arrow-up', 'Send', { cls: 'sm' }); send.type = 'submit';
      foot.append(input, send);
      const queue = el('div', 'queued tile-queue'); queue.hidden = true;
      t.append(head, bodyEl, queue, perm, foot);
      const tile = { el: t, run, queue, status, title, voice, who, pin, openBtn, bodyEl, convEl, perm, input, send, conv: new window.OverseerConversation(convEl, { post: m => post({ ...m, runId: m.runId || run.id, scope: 'tile' }), compact: true }) };
      title.addEventListener('click', e => { e.stopPropagation(); post({ type: 'track', runId: run.id }); });
      // A click anywhere on the tile (not on its controls or reply) tracks the agent (AC-105).
      t.addEventListener('click', e => { if (!e.target.closest('button, input, a, form, summary, .tile-perm')) post({ type: 'track', runId: run.id }); });
      openBtn.addEventListener('click', () => open(run.id));
      pin.addEventListener('click', () => post({ type: 'pin', runId: run.id, on: pin.getAttribute('aria-pressed') !== 'true' }));
      foot.addEventListener('submit', e => {
        e.preventDefault(); const text = input.value.trim(); if (!text || input.disabled) return;
        if (tile.asking) post({ type: 'permission', runId: run.id, request_id: tile.asking.request_id, allow: false, message: text, scope: 'tile' });
        else if (ACTIVE.has(tile.run.status) && tile.run.harness !== 'generic') post({ type: 'steer', runId: run.id, text, how: 'queue', scope: 'tile' });
        else post({ type: 'followUp', runId: run.id, text, scope: 'tile' });
        input.value = '';
      });
      t.addEventListener('keydown', e => {
        if (e.target !== t) return;
        if (e.key === 'Enter') { open(run.id); e.preventDefault(); }
        else if (/^Arrow/.test(e.key) && e.altKey) { place(run.id, e.key); e.preventDefault(); }
        else if (/^Arrow/.test(e.key)) { move(run.id, e.key); e.preventDefault(); }
      });
      return tile;
    }

    /** The nearest tile in an arrow's direction (by position on screen). */
    function neighbor(runId, key) {
      const from = tiles.get(runId)?.el.getBoundingClientRect(); if (!from) return undefined;
      const cx = r => r.left + r.width / 2, cy = r => r.top + r.height / 2;
      let best, bestD = Infinity;
      for (const [id, t] of tiles) {
        if (id === runId) continue;
        const r = t.el.getBoundingClientRect();
        const dx = cx(r) - cx(from), dy = cy(r) - cy(from);
        const ok = key === 'ArrowRight' ? dx > 1 : key === 'ArrowLeft' ? dx < -1 : key === 'ArrowDown' ? dy > 1 : dy < -1;
        if (!ok) continue;
        const d = key === 'ArrowRight' || key === 'ArrowLeft' ? Math.abs(dx) + 2 * Math.abs(dy) : Math.abs(dy) + 2 * Math.abs(dx);
        if (d < bestD) { bestD = d; best = id; }
      }
      return best;
    }
    function move(runId, key) { const next = tiles.get(neighbor(runId, key)); next && next.el.focus(); }
    /** Keyboard placement (AC-104): Alt+arrow puts the tile past its neighbor that way, or on the grid's edge. */
    function place(runId, key) {
      const edge = { ArrowRight: 'right', ArrowLeft: 'left', ArrowDown: 'bottom', ArrowUp: 'top' }[key];
      const target = neighbor(runId, key);
      drop(runId, target || null, edge);
      setTimeout(() => tiles.get(runId)?.el.focus(), 0);
    }

    function update(tile, run, state) {
      tile.run = run;
      window.OverseerChat.renderQueue(tile.queue, run.queue, m => post({ ...m, runId: run.id, scope: 'tile' }));
      tile.status.replaceChildren(ui.status(run.status, run.attention?.kind));
      tile.title.textContent = run.title; tile.title.title = `Open ${run.title}`;
      const p = run.profile_id && state.profiles.find(x => x.id === run.profile_id);
      // The account it runs on (AC-235): provider and plan, the shortened email.
      const acct = p ? (p.account && p.account.short) || p.name : '';
      tile.who.replaceChildren(ui.harnessMark(run.harness, 12), ...(acct ? [el('span', 'tile-account', acct)] : []));
      tile.who.title = [ui.HARNESS[run.harness] || run.harness, p ? (p.account && p.account.label) || p.name : '', run.model].filter(Boolean).join(' · ');
      const pinned = (state.pinned || []).includes(run.id);
      tile.pin.setAttribute('aria-pressed', String(pinned)); tile.pin.title = pinned ? 'Unpin' : 'Pin to grid'; tile.pin.setAttribute('aria-label', tile.pin.title);
      tile.el.classList.toggle('needs', run.status === 'waiting_for_user');
      // Oversight marks (AC-199): held, watched, watching, in conflict.
      const o = (state.overseer && state.overseer.run_id ? (state.oversight || {})[run.id] : undefined) || {};
      const marks = [o.held && ['debug-pause', 'held'], o.watched && ['eye', 'watched'], o.watching && o.watching.length && ['eye', 'watching'], o.conflicts && ['warning', `${o.conflicts} conflict${o.conflicts === 1 ? '' : 's'}`]].filter(Boolean);
      if (!tile.marks) { tile.marks = el('span', 'tile-marks'); tile.who.after(tile.marks); }
      tile.marks.replaceChildren(...marks.map(([icon, text]) => { const s = el('span', 'tile-mark'); s.append(ui.icon(icon, 'xs'), el('span', null, text)); return s; }));
      tile.el.classList.toggle('held', !!o.held); tile.el.classList.toggle('watched', !!o.watched); tile.el.classList.toggle('conflict', !!o.conflicts);
      // AC-243: what a finished agent's work became: "Merged into main (1a2b3c4)".
      const landed = !ACTIVE.has(run.status) && window.OverseerLanding ? window.OverseerLanding.text((state.landings || {})[run.workspace_id]) : '';
      if (!tile.landed) { tile.landed = el('span', 'tile-landed'); tile.marks.after(tile.landed); }
      tile.landed.hidden = !landed; tile.landed.textContent = landed; tile.landed.title = landed;
      tile.el.classList.toggle('merged', /^Merged/.test(landed));
      const att = run.attention && run.attention.kind === 'permission' ? run.attention : undefined;
      tile.perm.hidden = !att;
      if (att) {
        const d = window.OverseerConversation.describe(att.tool, att.input);
        const text = el('span', 'tile-perm-text', `Allow ${d.pending || d.verb} ${d.target || ''}?`); text.title = d.full || att.tool;
        tile.perm.replaceChildren(ui.icon('shield', 'sm'), text, ...window.OverseerConversation.permissionButtons(att.always, m => post({ type: 'permission', runId: run.id, request_id: att.request_id, ...m, scope: 'tile' }), { short: true }));
      }
      // A waiting agent always takes a reply (AC-241): with a permission waiting the reply denies it
      // with the reply as the reason; waiting on another question, it is queued for the agent.
      tile.asking = att;
      const waiting = run.status === 'waiting_for_user' || !!att;
      const busy = ACTIVE.has(run.status) && run.harness !== 'generic' && !waiting;
      tile.input.disabled = busy || (!att && String(run.capabilities?.follow_up || '').startsWith('unsupported'));
      tile.input.placeholder = att ? 'Why not?' : waiting ? 'Reply' : busy ? (window.OverseerContinuityText && window.OverseerContinuityText.isWaiting(run.status) ? 'Message for when it continues' : 'Working…') : 'Reply';
      tile.input.title = att ? 'Denies the request; the agent reads your note as the reason' : '';
    }

    let ready = false; // no layout (and no pruning of a restored one) before the first state arrives
    function layout(state) {
      if (!ready) return;
      for (const id of pinning) if ((state.pinned || []).includes(id)) pinning.delete(id);
      const wanted = [...pick(state).map(r => r.id), ...[...pinning].filter(id => state.runs.some(r => r.id === id))];
      const runs = new Map(state.runs.map(r => [r.id, r]));
      // Tiles whose agent left the grid go; a placed tile stays while its agent is running or pinned.
      for (const id of leaves(tree)) if (!wanted.includes(id) || !runs.has(id)) tree = without(tree, id);
      const aspect = (board.clientWidth || 16) / (board.clientHeight || 9);
      if (!custom) tree = balanced([...leaves(tree), ...wanted.filter(id => !leaves(tree).includes(id))].slice(0, MAX));
      else for (const id of wanted) if (!leaves(tree).includes(id) && leaves(tree).length < MAX) tree = autoPlace(tree, id, aspect, 1);
      order = leaves(tree);
      for (const [id, tile] of tiles) if (!order.includes(id)) { tile.el.remove(); tiles.delete(id); }
      for (const id of order) {
        const run = runs.get(id);
        let tile = tiles.get(id);
        if (!tile) { tile = makeTile(run); tiles.set(id, tile); markVoice(tile); }
        update(tile, run, state);
        tile.el.classList.toggle('tracked', id === tracked);
      }
      render();
      board.dataset.count = String(order.length);
      head.title = `Agent grid: ${order.length} of ${MAX} tiles`; head.setAttribute('aria-label', head.title);
      empty.hidden = true; board.hidden = order.length === 0;
      saveLayout?.({ tree, custom });
      // No empty grid (AC-113): the host takes the user home instead.
      if (visible && order.length === 0) post({ type: 'gridEmpty' });
      if (visible) post({ type: 'gridSubscribe', runIds: order });
    }

    /** Builds the split containers for the tree, moving (not recreating) each tile. */
    function render() {
      const build = n => {
        if (n.run) { const cell = el('div', 'cell'); cell.append(tiles.get(n.run).el); return cell; }
        const box = el('div', 'split split-' + n.dir); box.append(...n.parts.map(build)); return box;
      };
      const scrolls = new Map([...tiles].map(([id, t]) => [id, t.bodyEl.scrollTop]));
      board.replaceChildren(...(tree ? [build(tree)] : []));
      for (const [id, top] of scrolls) { const t = tiles.get(id); if (t) t.bodyEl.scrollTop = top; }
      board.dataset.layout = JSON.stringify(tree);
    }

    // ---- Drag and drop (AC-104) ----
    let dragging, dropAt;
    /** Where a drop at (x, y) goes: an edge of the grid (a band along its border) or of the tile under it. */
    function zone(x, y) {
      const b = board.getBoundingClientRect();
      const band = Math.min(28, b.width / 8, b.height / 8);
      // The nearest edge relative to the rectangle's size, so wide or tall tiles are fair on every side.
      const edgeOf = (r, px, py) => {
        const d = { left: (px - r.left) / r.width, right: (r.right - px) / r.width, top: (py - r.top) / r.height, bottom: (r.bottom - py) / r.height };
        return Object.entries(d).sort((a, c) => a[1] - c[1])[0][0];
      };
      if (x - b.left < band || b.right - x < band || y - b.top < band || b.bottom - y < band) return { target: null, edge: edgeOf(b, x, y), rect: b };
      for (const [id, t] of tiles) {
        const r = t.el.getBoundingClientRect();
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return { target: id, edge: edgeOf(r, x, y), rect: r };
      }
      return undefined;
    }
    function showDrop(z) {
      const host = board.getBoundingClientRect();
      const r = z.rect, half = { left: [r.left, r.top, r.width / 2, r.height], right: [r.left + r.width / 2, r.top, r.width / 2, r.height],
        top: [r.left, r.top, r.width, r.height / 2], bottom: [r.left, r.top + r.height / 2, r.width, r.height / 2] }[z.edge];
      Object.assign(preview.style, { left: half[0] - host.left + board.offsetLeft + 'px', top: half[1] - host.top + board.offsetTop + 'px', width: half[2] + 'px', height: half[3] + 'px' });
      preview.hidden = false; preview.dataset.edge = z.edge; preview.dataset.target = z.target || 'grid';
    }
    function hideDrop() { preview.hidden = true; full.hidden = true; dropAt = undefined; delete board.dataset.dropping; }
    const isFull = id => leaves(tree).length >= MAX && !leaves(tree).includes(id);
    board.addEventListener('dragover', e => {
      const types = [...(e.dataTransfer?.types || [])];
      const id = dragging || (types.includes('application/x-overseer-run') ? 'external' : undefined);
      if (!id) return;
      if (isFull(dragging)) { full.hidden = false; preview.hidden = true; e.dataTransfer.dropEffect = 'none'; return; }
      const z = zone(e.clientX, e.clientY);
      if (!z || z.target === dragging) { preview.hidden = true; dropAt = undefined; return; }
      e.preventDefault(); e.dataTransfer.dropEffect = 'move';
      dropAt = z; board.dataset.dropping = z.edge; showDrop(z);
    });
    board.addEventListener('dragleave', e => { if (!board.contains(e.relatedTarget)) hideDrop(); });
    board.addEventListener('drop', e => {
      e.preventDefault();
      const id = e.dataTransfer.getData('application/x-overseer-run') || dragging;
      const z = dropAt; hideDrop();
      if (id && z) drop(id, z.target, z.edge);
    });
    /** Puts `id` at `edge` of `target` (null: the grid), moving it if it is already a tile. */
    function drop(id, target, edge) {
      if (target === id) return;
      if (isFull(id)) { full.hidden = false; setTimeout(() => { full.hidden = true; }, 2500); return; }
      const known = leaves(tree).includes(id);
      tree = insert(without(tree, id), target && target !== id ? target : null, edge, id);
      custom = true;
      if (!known) { pinning.add(id); post({ type: 'pin', runId: id, on: true }); } // a placed agent stays in the grid
      layout(getState());
      post({ type: 'gridLayout', layout: tree });
    }

    return {
      open() { visible = true; layout(getState()); setTimeout(() => (tiles.get(order[0])?.el || startBtn).focus(), 0); },
      close() { if (!visible) return; visible = false; post({ type: 'gridSubscribe', runIds: [] }); },
      onState(state) { ready = true; renderRollup(state); if (visible) layout(state); },
      setTracked(runId) {
        tracked = runId || undefined;
        for (const [id, t] of tiles) { t.el.classList.toggle('tracked', id === tracked); t.el.setAttribute('aria-current', String(id === tracked)); }
        const run = tracked && getState().runs.find(r => r.id === tracked);
        trackBar.hidden = !tracked; trackText.textContent = run ? `Tracking ${run.title}` : '';
        board.dataset.tracked = tracked || '';
      },
      tracked: () => tracked,
      /** An agent dropped on the grid from the side bar (the host saw the drop, AC-104). */
      place(runId, edge) { if (runId && getState().runs.some(r => r.id === runId)) drop(runId, null, edge || 'right'); },
      layout: () => tree,
      /** Back to the balanced shapes (the Reset Grid Layout command). */
      reset() { custom = false; tree = balanced(leaves(tree)); if (visible) layout(getState()); },
      run(m) { const t = tiles.get(m.run.id); if (t) t.conv.setRun(m); },
      voiceTargets(list) { voiced = new Set(list); for (const t of tiles.values()) markVoice(t); },
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
