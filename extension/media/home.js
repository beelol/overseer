// Home (AC-182): with no agent selected, the conversation with Overseer sits above the composer.
// It is the daemon's one conversation (`overseer.session`): the owner's words, Overseer's replies,
// the cards (an agent started, a report, a question and its answer, a claim, a finding, a check-in
// that found an agent done, a watch, a share withdrawn) and the proposals that wait for a yes.
// The docked chat (AC-107) shows the same conversation from the other side.
(function () {
  const ui = window.OverseerUI, el = ui.el;
  const CARD_ICON = { started: 'rocket', report: 'note', ask: 'question', answer: 'comment', claim: 'symbol-folder', done: 'check', finding: 'eye', watch: 'eye', watch_ended: 'eye-closed', withdrawn: 'discard', cannot_answer: 'warning', hold: 'debug-pause', release: 'debug-continue', conflict: 'warning', check_in: 'checklist' };

  function create(host, { post, startWith }) {
    const wrap = el('section', 'home'); wrap.setAttribute('aria-label', 'Conversation with Overseer'); wrap.id = 'home';
    const head = el('header', 'home-head');
    head.append(ui.mark('sm'), el('span', 'home-title', 'Overseer'));
    const level = el('span', 'home-level'); level.id = 'home-level';
    const fresh = el('button', 'link', 'Start fresh'); fresh.type = 'button'; fresh.id = 'home-fresh'; fresh.title = 'Archive this conversation and begin a new one (holds, guardrails, areas, conflicts and watches stay)';
    fresh.addEventListener('click', () => post({ type: 'overseerFresh' }));
    head.append(el('span', 'spacer'), level, fresh);
    const list = el('div', 'home-list'); list.id = 'home-conv'; list.setAttribute('role', 'log'); list.setAttribute('aria-live', 'polite'); list.setAttribute('aria-label', 'Conversation with Overseer');
    const empty = el('div', 'home-empty', 'Nothing yet. Type a task to start an agent, or @overseer to ask what your agents are doing.');
    wrap.append(head, list, empty);
    host.prepend(wrap);
    let session, shown = new Map(); // message id -> element

    function row(m) {
      const src = m.source || 'system';
      if (m.card) { const c = card(m); c.dataset.id = m.id; return c; }
      const r = el('div', `home-msg from-${src}`); r.dataset.id = m.id;
      const who = el('div', 'home-from');
      if (src === 'overseer') who.append(ui.mark('sm'), el('span', null, 'Overseer'));
      else if (src === 'owner') who.append(ui.icon('account', 'xs'), el('span', null, 'You'));
      else who.append(ui.icon('info', 'xs'), el('span', null, src));
      const text = el('div', 'home-text', m.text || '');
      r.append(who, text);
      if (src === 'owner') {
        // A message meant for an agent: start one from it (AC-182's correction).
        const b = el('button', 'link', 'Start as an agent'); b.type = 'button'; b.dataset.action = 'start-as-agent';
        b.addEventListener('click', () => startWith(m.text || ''));
        r.append(b);
      }
      return r;
    }

    function card(m) {
      const c = m.card, kind = c.kind || 'card';
      const r = el('div', `home-card card card-${kind}`); r.dataset.kind = kind; r.setAttribute('role', 'group');
      const head = el('div', 'card-head');
      head.append(ui.icon(CARD_ICON[kind] || 'info', 'sm'), el('span', 'card-text', m.text || kind));
      r.append(head);
      const detail = [];
      if (kind === 'report') { if (c.needs) detail.push(`Needs: ${c.needs}`); if (c.blocked) detail.push(`Blocked by: ${c.blocked}`); if (c.changed && c.changed.length) detail.push(`Changed: ${c.changed.join(', ')}`); }
      if (kind === 'ask') detail.push(c.answer ? `Answer: ${c.answer}` : 'Waiting for an answer');
      if (kind === 'answer') detail.push(`Question: ${c.question}`);
      if (kind === 'done') { if (c.done) detail.push(c.done); if (c.left_out) detail.push(`Left out: ${c.left_out}`); }
      if (kind === 'finding') { detail.push(`${c.result}${c.held ? ' · held at once' : ''}${c.snapshot ? ' · snapshot ' + String(c.snapshot).slice(0, 8) : ''}`); }
      if (kind === 'started') { if (c.prompt) detail.push(ui.firstLine(c.prompt, 160)); }
      if (kind === 'cannot_answer' && c.reason) detail.push(c.reason);
      if (detail.length) r.append(el('div', 'card-detail', detail.join(' · ')));
      if (kind === 'started' && c.agent) {
        // Meant for Overseer after all: stop the agent just started and ask instead.
        const b = el('button', 'link', 'Ask Overseer instead'); b.type = 'button'; b.dataset.action = 'ask-overseer-instead';
        b.addEventListener('click', () => { b.disabled = true; post({ type: 'overseerUndoStart', runId: c.agent, text: c.prompt || '' }); });
        r.append(b);
      }
      return r;
    }

    function proposal(p) {
      const card = el('div', 'proposal'); card.setAttribute('role', 'group'); card.setAttribute('aria-label', 'Overseer proposes'); card.dataset.id = p.id;
      const head = el('div', 'proposal-head'); head.append(ui.mark('sm'), el('span', null, 'Overseer will'));
      const list = el('ul', 'proposal-list');
      for (const line of p.lines || (p.actions || []).map(a => a.action + (a.title ? ' ' + a.title : ''))) list.append(el('li', null, line));
      const status = el('div', 'proposal-status'); status.setAttribute('role', 'status');
      const yes = el('button', 'btn primary sm', 'Yes'); yes.type = 'button'; yes.dataset.proposal = 'yes';
      const no = el('button', 'btn sm', 'No'); no.type = 'button'; no.dataset.proposal = 'no';
      const row = el('div', 'proposal-actions'); row.append(yes, no);
      card.append(head, list, row, status);
      const decide = ok => { yes.disabled = no.disabled = true; status.textContent = ok ? 'Working…' : 'Declining…'; post({ type: 'overseerAnswer', id: p.id, yes: ok }); };
      yes.addEventListener('click', () => decide(true));
      no.addEventListener('click', () => decide(false));
      if (p.state && p.state !== 'open') { status.textContent = p.result || p.state; row.hidden = true; card.classList.add('answered'); }
      return card;
    }

    return {
      /** The daemon's session: redrawn from its messages and open proposals (cheap: ids are stable). */
      session(s) {
        session = s;
        const messages = (s && s.messages) || [];
        const open = ((s && s.proposals) || []).filter(p => p.state === 'open' || p.state === 'settling');
        level.textContent = s && s.level ? { ask_first: 'Ask first', steer: 'Steer', auto: 'Auto' }[s.level] || s.level : '';
        const keep = new Set();
        for (const m of messages) {
          keep.add(m.id);
          let e = shown.get(m.id);
          if (!e) { e = row(m); shown.set(m.id, e); list.append(e); }
          else if (m.card && m.card.kind === 'ask' && e.dataset.answer !== String(m.card.answer || '')) { const n = row(m); e.replaceWith(n); shown.set(m.id, n); e = n; }
          if (m.card && m.card.kind === 'ask') e.dataset.answer = String(m.card.answer || '');
        }
        for (const p of open) {
          const id = 'p:' + p.id; keep.add(id);
          if (!shown.has(id)) { const e = proposal(p); shown.set(id, e); list.append(e); }
        }
        for (const [id, e] of shown) if (!keep.has(id)) { e.remove(); shown.delete(id); }
        empty.hidden = messages.length > 0 || open.length > 0;
        list.scrollTop = list.scrollHeight;
      },
      /** A message came back for a proposal card (an error, a state). */
      proposalStatus(id, text) { const e = shown.get('p:' + id); if (e) e.querySelector('.proposal-status').textContent = text; },
      get current() { return session; },
    };
  }
  window.OverseerHome = { create };
})();
