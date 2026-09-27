// Continuity in the webviews (Gate L): what a chat, a grid tile, the composer and the New Task
// form show when the connection changes. The words come from continuity-text.js; this file draws
// them. It hooks into the shared scripts in a few places and changes nothing when the daemon
// reports no Continuity data (an older daemon): every hook then returns "not mine".
//
//   chat and tiles  transition notes, one quiet card while an agent waits (Use a local model
//                   now, Retry now, Stop), the way back when the connection returns
//   composer        local models under Local with a fit badge, online agents disabled while
//                   offline with the reason, the first-use notice (once per machine)
(function () {
  const ui = window.OverseerUI, el = ui.el, T = window.OverseerContinuityText;
  if (!ui || !T) return;
  const LOCAL = 'opencode-serve', LOCAL_ACCOUNT = 'local-ollama';
  let data = null;          // the daemon's `continuity.ui`
  const convs = new Set();  // conversations on this page
  const watchers = new Set();

  // ---- Names and states the shared helpers did not know ----
  ui.HARNESS[LOCAL] = T.HARNESS[LOCAL];
  const statusText = ui.statusText, status = ui.status;
  ui.statusText = s => (T.STATES[s] ? T.STATES[s].text : statusText(s));
  ui.status = (s, attention) => {
    if (!T.STATES[s]) return status(s, attention);
    const e = el('span', `status st-${s}`); e.setAttribute('role', 'img'); e.setAttribute('aria-label', T.STATES[s].text); e.title = T.STATES[s].text;
    e.append(ui.icon(T.STATES[s].icon, 'sm'));
    return e;
  };

  /** Text with **bold** parts, as elements (never as HTML). */
  function rich(text) {
    const out = [];
    String(text || '').split('**').forEach((part, i) => { if (part) out.push(i % 2 ? el('strong', null, part) : document.createTextNode(part)); });
    return out;
  }
  function note(icon, text, cls = '') {
    const n = el('div', `sys cont-note ${cls}`.trim());
    n.append(ui.icon(icon, 'sm'), Object.assign(el('span'), { }));
    n.lastChild.append(...rich(text));
    return n;
  }
  function button(action, post, extra = {}) {
    const b = el('button', 'btn sm' + (action.primary ? ' primary' : ''), action.label); b.type = 'button';
    b.dataset.continuity = action.id + (action.to ? ':' + action.to : '');
    if (action.detail) b.title = action.detail;
    b.addEventListener('click', () => { b.disabled = true; setTimeout(() => { b.disabled = false; }, 4000); post({ type: 'continuity', action: action.id, to: action.to, ...extra }); });
    return b;
  }
  function card(kind, c, post, extra) {
    const box = el('div', `cont-card ${kind}`); box.dataset.continuityCard = kind; box.setAttribute('role', 'status');
    const head = el('div', 'cont-head'); head.append(ui.icon(c.icon || 'cloud', 'sm'), el('span', 'cont-title', c.title));
    box.append(head);
    if (c.line) box.append(el('div', 'cont-line', c.line));
    if (c.note) box.append(el('div', 'cont-why', c.note.replace(/^./, m => m.toUpperCase()) + (/[.!?]$/.test(c.note) ? '' : '.')));
    for (const a of c.actions.filter(a => a.detail)) box.append(el('div', 'cont-why', `${a.label}: ${a.detail}`));
    if (c.actions.length) { const row = el('div', 'cont-actions'); row.append(...c.actions.map(a => button(a, post, extra))); box.append(row); }
    if (c.foot) box.append(el('div', 'cont-foot', c.foot));
    return box;
  }

  // ---- Conversations ----
  /** What this conversation keeps for Continuity. */
  function of(conv) {
    if (!conv.continuity) { conv.continuity = { wait: null, waitEl: null, back: null, backEl: null, model: null, seen: new Set() }; convs.add(conv); }
    return conv.continuity;
  }
  const post = conv => m => conv.opts.post && conv.opts.post(m);
  const last = conv => conv.turn().body;

  function drawWait(conv) {
    const c = of(conv);
    // The daemon's own record and what the events said: the events are newer when both exist.
    const live = data && (data.waiting || []).find(w => w.run_id === conv.rootId);
    const wait = live || c.wait ? { ...(live || {}), ...(c.wait || {}), offers: (c.wait && c.wait.offers && c.wait.offers.length ? c.wait.offers : (live && live.offers)) || [], next_ms: Math.max((live && live.next_ms) || 0, (c.wait && c.wait.next_ms) || 0) || undefined, note: (c.wait && c.wait.note) ?? (live && live.note) } : null;
    const waiting = T.isWaiting(conv.status);
    if (c.waitEl) { c.waitEl.remove(); c.waitEl = null; }
    if (!wait || !waiting) return;
    c.waitEl = card('waiting', T.waitCard(wait, data, Date.now()), post(conv));
    last(conv).append(c.waitEl);
  }
  function drawBack(conv) {
    const c = of(conv);
    if (c.backEl) { c.backEl.remove(); c.backEl = null; }
    if (!c.back || conv.status === 'handed_off') return;
    const b = T.backCard(c.back);
    if (!b.line && !b.actions.length) return;
    c.backEl = card('back', b, post(conv));
    last(conv).append(c.backEl);
  }

  /** The handoff prompt a successor starts from is the daemon's, not the user's: it folds behind one line. */
  function fold(conv) {
    for (const text of conv.list.querySelectorAll('.msg.user > .text')) {
      if (text.dataset.folded || !/^You are continuing a task another agent started; /.test(text.textContent)) continue;
      text.dataset.folded = '1';
      const d = el('details', 'cont-fold'); const s = el('summary', null, 'What the agent was told at the handoff'); s.title = 'The prompt Overseer built from its own records: the task, the worktree, what was done, and what is pending';
      const body = el('div', 'text', text.textContent);
      d.append(s, body);
      text.replaceWith(d);
      text.closest('.msg').setAttribute('aria-label', 'Handoff');
    }
  }

  /** A conversation learned about its run (status, attention). */
  function run(conv, msg) {
    of(conv);
    conv.status = msg.run.status;
    if (T.STATES[msg.run.status]) { conv.active = false; conv.updateWorking(); }
    drawWait(conv); drawBack(conv);
  }

  /** One daemon event. Returns true when it was drawn (or deliberately left out) here. */
  function event(conv, ev) {
    const p = ev.payload || {};
    const mine = !ev.run_id || ev.run_id === conv.rootId;
    const c = of(conv);
    switch (ev.kind) {
      case 'output':
        if (!(p.role === 'system' && p.continuity)) return false;
        if (mine) last(conv).append(note(/^Back online/.test(p.text) ? 'cloud' : /^Queued/.test(p.text) ? 'clock' : /^Memory/.test(p.text) ? 'chip' : 'arrow-right', p.text));
        return true;
      case 'status':
        if (!mine) return false;
        conv.status = p.status;
        if (T.isWaiting(p.status)) {
          // The turn is not over: its message is kept and will be sent again.
          const t = conv.turns.length ? conv.turn() : null;
          if (t) { t.foot.hidden = true; t.ended = false; }
          c.wait = { run_id: conv.rootId, kind: p.status === 'waiting_for_memory' ? 'memory' : 'connection', reason: p.reason, since_ms: ev.ts_ms || ev.ts, offers: [] };
          conv.active = false; conv.updateWorking(); drawWait(conv);
          return true;
        }
        if (p.status === 'handed_off') { conv.active = false; conv.updateWorking(); drawWait(conv); drawBack(conv); return true; }
        drawWait(conv);
        return false;
      case 'retry':
        if (!mine) return true;
        if (p.sending) { c.wait = null; drawWait(conv); last(conv).append(note('cloud', 'The connection is back. Sending your message again.')); return true; }
        if (p.retry_now) return true;
        c.wait = { ...(c.wait || { run_id: conv.rootId, kind: 'connection' }), reason: p.reason || (c.wait && c.wait.reason), next_ms: (ev.ts_ms || ev.ts || Date.now()) + (p.next_in_ms || 0), note: p.note, offers: p.offers || [], attempts: p.attempt };
        drawWait(conv);
        return true;
      case 'handoff': {
        if (!mine) return true;
        const away = p.predecessor === conv.rootId;
        const other = away ? p.successor : p.predecessor;
        const n = note('arrow-right', away ? 'The work continues in another agent.' : 'This agent continues the work of another.');
        const open = el('button', 'link', away ? 'Open it' : 'Open the first agent'); open.type = 'button'; open.dataset.continuity = 'open';
        open.addEventListener('click', () => post(conv)({ type: 'continuity', action: 'open', target: other }));
        n.append(open);
        last(conv).append(n);
        return true;
      }
      case 'error': {
        // A connection error reported twice by the harness (its error line and its failed turn) is shown once.
        if (!mine || p.class !== 'network') return false;
        const key = String(p.message || '');
        if (c.lastError === key) return true;
        c.lastError = key;
        return false;
      }
      case 'stall': if (mine) last(conv).append(note('debug-pause', 'No answer while offline. Overseer interrupted the turn; your message is kept.')); return true;
      case 'memory_valve': if (mine) last(conv).append(note('chip', 'The system ran short of memory. Overseer paused this agent and unloaded the model; your message is kept.')); return true;
      case 'local_model': {
        if (!mine) return true;
        const key = `${p.model}@${p.context}`;
        if (c.model !== key) {
          c.model = key;
          const bytes = typeof p.bytes === 'number' ? ` · ${T.gib(p.bytes)} GiB` : '';
          last(conv).append(note('server', `Local model **${String(p.base || p.model).replace(/^ollama\//, '')}** at a ${Math.round((p.context || 0) / 1024)}k context${bytes}${p.already_loaded ? ', already loaded' : ''}.`));
        }
        return true;
      }
      case 'back_online':
        if (!mine) return true;
        c.back = p.stay ? { stay: true } : p;
        drawBack(conv);
        return true;
      case 'attention':
        if (mine && p.kind && p.reason) last(conv).append(note('cloud', `${String(p.reason).replace(/^./, m => m.toUpperCase())}. Your message is kept.`));
        return true;
      case 'local_load': case 'local_queue': case 'local_download': case 'ollama_install': case 'ollama_server': case 'connection': case 'continuity_settings':
        return true;
      default:
        // Whatever comes next ends a wait that was drawn.
        if (mine && (ev.kind === 'turn_started' || ev.kind === 'turn_done')) { if (ev.kind === 'turn_started') { c.wait = null; c.lastError = undefined; } setTimeout(() => { fold(conv); drawWait(conv); drawBack(conv); }, 0); }
        return false;
    }
  }

  // ---- The composer ----
  const current = d => data || (d && d.continuity) || null;
  const isLocal = form => form.harness === LOCAL;
  function useLocal(form, tag) { form.harness = LOCAL; form.account = LOCAL_ACCOUNT; form.model = tag || ''; }
  function badge(b, markToo) {
    const out = [Object.assign(el('span', `cont-badge ${b.tone}`, b.badge), { title: b.detail })];
    if (markToo && b.mark) out.push(Object.assign(el('span', 'cont-badge mark', b.mark), { title: b.detail }));
    return out;
  }

  /** The Agent menu: the local harness becomes "Local models", and what cannot be reached says why. */
  function agentMenu(items, { data: d, form, save }) {
    const c = current(d);
    if (!c) return;
    // The raw harness entries of the local transport are replaced by the models themselves.
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i];
      if (it && it.head && /^(Local model|opencode-serve)/.test(it.head)) { let n = 1; while (items[i + n] && !items[i + n].head && items[i + n] !== 'sep') n++; items.splice(i, n); }
    }
    let head;
    for (const it of items) {
      if (it === 'sep') continue;
      if (it.head) { head = it.head; continue; }
      const harness = Object.keys(ui.HARNESS).find(h => head && head.startsWith(ui.HARNESS[h]));
      const why = harness && T.harnessBlocked(harness, c);
      if (why) { it.disabled = true; it.why = why; it.hint = 'offline'; }
    }
    const choices = T.localChoices(c);
    items.push({ head: `Local models · Ollama${choices.running ? '' : ' · not running'}` });
    if (!choices.running) { items.push({ label: choices.ollama, icon: 'circle-slash', disabled: true, why: choices.ollama }); return; }
    items.push({ label: choices.pick ? `Best fit · ${choices.pick.tag}` : 'Best fit', logo: ui.harnessMark('opencode', 14), hint: choices.pick ? `${Math.round(choices.pick.context / 1024)}k` : 'none fits', checked: isLocal(form) && !form.model,
      disabled: !choices.pick, why: choices.why_no_pick, title: choices.pick ? 'Overseer picks the best verified model that fits the memory free now' : choices.why_no_pick, id: 'local-best', run: () => { form.chosenHere = true; useLocal(form, ''); save(); } });
    for (const m of choices.models.filter(m => m.installed)) {
      items.push({ label: m.tag, logo: ui.harnessMark('opencode', 14), hint: [m.badge.badge, m.badge.mark].filter(Boolean).join(' · '), checked: isLocal(form) && form.model === `ollama/${m.tag}`, disabled: !m.badge.usable, why: m.badge.detail, title: m.badge.detail,
        id: 'local-' + m.tag.replace(/[^a-z0-9]+/gi, '-'), run: () => { form.chosenHere = true; useLocal(form, `ollama/${m.tag}`); save(); } });
    }
    const missing = choices.models.filter(m => !m.installed);
    if (missing.length) items.push({ label: `${missing.length} more not installed…`, icon: 'cloud-download', title: missing.map(m => `${m.tag}: ${m.badge.badge}`).join('\n'), run: () => window.OverseerContinuity.post({ type: 'command', command: 'overseer.continuity.localModels' }) });
  }

  /** The Model menu of a local agent: the models, with their badges. Returns true when it opened. */
  function modelMenu(anchor, { data: d, form, save }) {
    if (!isLocal(form)) return false;
    const c = current(d); const choices = T.localChoices(c);
    ui.menu(anchor, [{ label: choices.pick ? `Best fit · ${choices.pick.tag}` : 'Best fit', icon: 'sparkle', checked: !form.model, disabled: !choices.pick, why: choices.why_no_pick, run: () => { form.model = ''; save(); } },
      ...choices.models.filter(m => m.installed).map(m => ({ label: m.tag, icon: 'server', hint: [m.badge.badge, m.badge.mark].filter(Boolean).join(' · '), checked: form.model === `ollama/${m.tag}`, disabled: !m.badge.usable, why: m.badge.detail, title: m.badge.detail, run: () => { form.model = `ollama/${m.tag}`; save(); } }))], { label: 'Local model' });
    return true;
  }

  /** What stops a new agent, as the composer shows problems; undefined when nothing of Continuity does. */
  function problem({ data: d, form, save, task }) {
    const c = current(d);
    if (!c) return undefined;
    ensureNotice(c);
    const choices = T.localChoices(c);
    // Back online: what was chosen before the connection dropped is chosen again, whenever the composer looks.
    if (form.beforeOffline && !T.harnessBlocked(form.beforeOffline.harness, c)) { Object.assign(form, form.beforeOffline); form.beforeOffline = undefined; if (save) setTimeout(save, 0); return undefined; }
    if (isLocal(form)) {
      form.account = LOCAL_ACCOUNT;
      if (d && !d.trusted) return undefined;
      if (!form.repo) return undefined;
      const hx = d && (d.harnesses || []).find(h => h.harness === LOCAL);
      if (hx && !hx.installed) return { text: 'OpenCode is not installed; local models run in it.', fix: 'How to install', url: 'https://opencode.ai/docs' };
      if (!choices.running) return { text: `${choices.ollama}.`, fix: c.settings && c.settings.allowOllamaInstall ? undefined : 'Allow Overseer to start it', action: () => window.OverseerContinuity.post({ type: 'command', command: 'overseer.continuity.act', args: { action: 'allow', setting: 'allowOllamaInstall' } }) };
      const chosen = form.model && choices.models.find(m => `ollama/${m.tag}` === form.model);
      if (form.model && !chosen) return { text: `${form.model.replace(/^ollama\//, '')} is not installed in Ollama.`, fix: 'Use the best fit', action: () => { form.model = ''; save(); } };
      if (chosen && !chosen.badge.usable) return { text: chosen.badge.detail, fix: choices.pick ? `Use ${choices.pick.tag}` : undefined, action: () => { form.model = ''; save(); } };
      if (!form.model && !choices.pick) return { text: `No local model can run now${choices.why_no_pick ? ` (${choices.why_no_pick})` : ''}.` };
      // A local agent has no account to choose or sign in to: only the task is still asked for.
      return String(task || '').trim() ? {} : { soft: true };
    }
    const why = T.harnessBlocked(form.harness, c);
    if (why) {
      return { text: `${why}. ${ui.HARNESS[form.harness] || form.harness} cannot be reached.`, fix: choices.pick ? `Use a local model (${choices.pick.tag})` : undefined, action: () => { form.beforeOffline = { harness: form.harness, account: form.account, model: form.model }; useLocal(form, ''); save(); } };
    }
    return undefined;
  }

  /** When the composer opens: back online, the last online agent is the default again; offline, the best local model is. */
  function defaults({ data: d, form }) {
    const c = current(d);
    const na = c && c.new_agents;
    if (!na || !na.default) return;
    if (isLocal(form) && !na.default.local && !form.chosenHere) {
      const accounts = ((d && d.accounts) || []).filter(a => (a.harnesses || []).includes(na.default.harness));
      form.harness = na.default.harness; form.account = na.default.profile_id || (accounts.find(a => a.signedIn) || accounts[0] || {}).id; form.model = '';
    } else if (!isLocal(form) && na.local_only && T.localChoices(c).pick) {
      form.beforeOffline = { harness: form.harness, account: form.account, model: form.model };
      useLocal(form, '');
    }
  }

  /** The chips of a local agent: its name and its model, with the badge in the tooltip. */
  function chips({ data: d, form, agentChip, modelChip, setChip }) {
    const c = current(d);
    drawBanner(c);
    if (!c || !isLocal(form)) return;
    const choices = T.localChoices(c);
    const chosen = form.model ? choices.models.find(m => `ollama/${m.tag}` === form.model) : null;
    const tag = chosen ? chosen.tag : choices.pick ? choices.pick.tag : '';
    setChip(agentChip, ui.harnessMark('opencode', 14), 'Local model', `A local model through Ollama and OpenCode.\nNo account, no network, no cost.\n${choices.ollama}`);
    setChip(modelChip, 'server', chosen ? chosen.tag : tag ? `Best fit · ${tag}` : 'No model fits', chosen ? chosen.badge.detail : choices.pick ? `${choices.pick.tag} at a ${Math.round(choices.pick.context / 1024)}k context: the best verified model that fits the memory free now` : choices.why_no_pick);
  }

  // ---- The notice and the offline line above the composer ----
  let noticeEl, bannerEl;
  function anchor() { return document.querySelector('.composer-hero .composer.big') || document.querySelector('main.form #error'); }
  function drawBanner(c) {
    const at = anchor(); if (!at) return;
    if (!bannerEl) { bannerEl = el('div', 'cont-banner'); bannerEl.setAttribute('role', 'status'); bannerEl.dataset.continuity = 'banner'; }
    const conn = c && T.connection(c);
    const show = !!conn && conn.state !== 'online' && conn.state !== 'unknown';
    // A download in progress is shown here too, with Cancel, whoever asked for it.
    const active = ((c && c.downloads && c.downloads.downloads) || []).map(T.download).filter(x => x.active);
    bannerEl.hidden = !show && !active.length;
    bannerEl.replaceChildren();
    if (show) { const line = el('span'); line.append(ui.icon('cloud', 'sm'), el('span', null, `${conn.sentence} ${T.policy(c)}`)); line.title = conn.lines.join('\n'); bannerEl.append(line); }
    for (const [i, d] of active.entries()) {
      const raw = c.downloads.downloads.filter(x => T.download(x).active)[i];
      const line = el('span', 'cont-download'); line.dataset.continuity = 'download:' + raw.tag;
      const cancel = el('button', 'link', 'Cancel'); cancel.type = 'button'; cancel.addEventListener('click', () => window.OverseerContinuity.post({ type: 'command', command: 'overseer.continuity.act', args: { action: 'cancel_pull', tag: raw.tag } }));
      line.append(ui.icon('cloud-download', 'sm'), el('span', null, d.text), cancel);
      bannerEl.append(line);
    }
    if (bannerEl.parentNode !== at.parentNode || bannerEl.nextSibling !== at) at.before(bannerEl);
  }
  let noticeKey;
  function ensureNotice(c) {
    const at = anchor(); if (!at) return;
    const n = T.notice(c);
    if (!n) { if (noticeEl) { noticeEl.remove(); noticeEl = null; noticeKey = undefined; } return; }
    const key = JSON.stringify(n);
    if (noticeEl && noticeEl.isConnected && key === noticeKey) return;
    noticeKey = key;
    const box = el('section', 'cont-notice'); box.dataset.continuity = 'notice'; box.setAttribute('aria-label', n.title);
    const head = el('div', 'cont-head'); head.append(ui.icon('cloud', 'sm'), el('span', 'cont-title', n.title));
    const switches = el('div', 'cont-switches');
    for (const s of n.switches) {
      const row = el('div', 'cont-switch'); row.dataset.setting = s.setting;
      row.append(el('span', 'name', s.label), el('span', 'state' + (s.on ? ' on' : ''), s.on ? 'allowed' : 'off'));
      if (!s.on) { const allow = el('button', 'link', 'Allow'); allow.type = 'button'; allow.dataset.continuity = 'allow:' + s.setting; allow.title = s.detail;
        allow.addEventListener('click', () => window.OverseerContinuity.post({ type: 'command', command: 'overseer.continuity.act', args: { action: 'allow', setting: s.setting } })); row.append(allow); }
      row.append(el('span', 'detail', s.on ? '' : s.detail));
      switches.append(row);
    }
    const actions = el('div', 'cont-actions');
    for (const a of n.actions) {
      const b = el('button', 'btn sm' + (a.primary ? ' primary' : ''), a.label); b.type = 'button'; b.dataset.continuity = a.id;
      b.addEventListener('click', () => window.OverseerContinuity.post({ type: 'command', command: 'overseer.continuity.act', args: { action: a.id } }));
      actions.append(b);
    }
    box.append(head, el('div', 'cont-line', n.text), switches, actions);
    if (noticeEl) noticeEl.replaceWith(box); else at.before(box);
    noticeEl = box;
  }

  // ---- Data from the host ----
  function set(next) {
    data = next || null;
    for (const conv of convs) { if (!conv.root.isConnected) { convs.delete(conv); continue; } drawWait(conv); }
    drawBanner(data); ensureNotice(data);
    for (const w of watchers) { try { w(data); } catch { /* a view that went away */ } }
    document.body.dataset.connection = data && data.connection ? data.connection.state : '';
  }
  window.addEventListener('message', e => { const m = e.data; if (m && m.type === 'continuity') set(m.data); });
  // The countdown of a waiting card moves on by itself.
  setInterval(() => { for (const conv of convs) if (conv.continuity && conv.continuity.waitEl) drawWait(conv); }, 5000);

  window.OverseerContinuity = {
    LOCAL, LOCAL_ACCOUNT, event, run, agentMenu, modelMenu, problem, chips, defaults, set, badge,
    data: () => data, choices: d => T.localChoices(current(d)), blocked: (harness, d) => T.harnessBlocked(harness, current(d)),
    watch: fn => { watchers.add(fn); return () => watchers.delete(fn); },
    /** Messages that are not about one run go through the page's own channel to the host. */
    post: m => { const api = window.overseerApi; if (api) api.postMessage(m); },
  };
})();
