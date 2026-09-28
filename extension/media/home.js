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
    // Voice Mode's strip (AC-174): the state, the words as they are heard, mute, and the voice view.
    const vstrip = el('div', 'home-voice'); vstrip.id = 'home-voice'; vstrip.hidden = true;
    const vState = el('span', 'home-voice-state'); vState.setAttribute('role', 'status');
    const vHeard = el('span', 'home-voice-heard'); vHeard.setAttribute('aria-live', 'polite');
    const vMute = ui.iconButton('mic', 'Mute Voice Mode', { cls: 'sm', pressed: false }); vMute.id = 'home-voice-mute';
    vMute.addEventListener('click', () => post({ type: 'voiceMute' }));
    const vShow = ui.iconButton('screen-full', 'Show Voice Mode', { cls: 'sm' }); vShow.id = 'home-voice-show';
    vShow.addEventListener('click', () => post({ type: 'voiceShow' }));
    vstrip.append(vState, vHeard, vMute, vShow);
    head.append(vstrip, el('span', 'spacer'), level, fresh);
    const list = el('div', 'home-list'); list.id = 'home-conv'; list.setAttribute('role', 'log'); list.setAttribute('aria-live', 'polite'); list.setAttribute('aria-label', 'Conversation with Overseer');
    wrap.append(head, list);
    wrap.hidden = true;
    host.prepend(wrap);
    let session, shown = new Map(); // message id -> element

    function row(m) {
      const src = m.source || 'system';
      if (m.card) { const c = card(m); c.dataset.id = m.id; return c; }
      const r = el('div', `home-msg from-${src}`); r.dataset.id = m.id;
      const who = el('div', 'home-from');
      // A spoken request (Voice Mode): the owner's words, not the notes the daemon adds for Overseer.
      const spoken = src === 'owner' && m.surface === 'voice' && /Request (V-\d+): ([\s\S]*)$/.exec(m.text || '');
      if (src === 'overseer') who.append(ui.mark('sm'), el('span', null, 'Overseer'));
      else if (spoken) { const mic = ui.icon('mic', 'xs'); who.append(mic, el('span', null, `You, by voice · ${spoken[1]}`)); r.classList.add('spoken'); }
      else if (src === 'owner') who.append(ui.icon('account', 'xs'), el('span', null, 'You'));
      else who.append(ui.icon('info', 'xs'), el('span', null, src));
      const text = el('div', 'home-text', spoken ? spoken[2].trim() : m.text || '');
      r.append(who, text);
      if (src === 'owner' && !spoken) {
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
      if (p.cause === 'voice') { const mic = ui.icon('mic', 'xs'); mic.removeAttribute('aria-hidden'); mic.setAttribute('role', 'img'); mic.setAttribute('aria-label', 'From a spoken request'); mic.title = 'From a spoken request'; head.append(mic); card.classList.add('spoken'); }
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

    // An answered proposal (AC-185): what Overseer did, with one row per agent: why it was chosen,
    // the delivery, the state and when it got there; the whole text sent is the row's tooltip.
    const STATE = { yes: 'Done', done: 'Done', no: 'Declined', cancelled: 'Cancelled', stale: 'Not done', not_done: 'Not done', failed: 'Failed', refused: 'Refused' };
    const ROW = { held: 'held', sent: 'sent', delivered: 'delivered', picked_up: 'picked up', answered: 'answered', failed: 'failed', cancelled: 'cancelled', not_sent: 'not sent' };
    const clock = ms => ms ? new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : '';
    function answered(c) {
      const card = el('div', 'proposal answered done-card'); card.setAttribute('role', 'group'); card.setAttribute('aria-label', 'What Overseer did'); card.dataset.id = c.id; card.dataset.state = c.state;
      const head = el('div', 'proposal-head'); head.append(ui.mark('sm'), el('span', null, STATE[c.state] || c.state));
      const by = [c.answered_by, c.surface && c.surface !== 'settle' ? c.surface : ''].filter(Boolean).join(' · ');
      if (by) head.append(el('span', 'card-by', by));
      const list = el('ul', 'proposal-list');
      for (const line of c.lines || []) list.append(el('li', null, line));
      card.append(head, list);
      if ((c.rows || []).length) {
        const rows = el('div', 'card-rows'); rows.setAttribute('role', 'list');
        for (const r of c.rows) {
          const row = el('div', 'card-row'); row.setAttribute('role', 'listitem'); row.dataset.state = r.state; row.dataset.run = r.run_id;
          const at = r.answered_ms || r.picked_ms || r.delivered_ms || r.sent_ms || r.held_ms;
          const times = [['held', r.held_ms], ['sent', r.sent_ms], ['delivered', r.delivered_ms], ['picked up', r.picked_ms], ['answered', r.answered_ms]].filter(x => x[1]).map(([k, v]) => `${k} ${clock(v)}`).join(' · ');
          row.append(el('span', 'card-row-agent', r.title || r.run_id), el('span', 'card-row-meta', [r.why, r.delivery].filter(Boolean).join(' · ')), el('span', `card-row-state state-${r.state}`, ROW[r.state] || r.state), el('span', 'card-row-time', clock(at)));
          row.title = `${r.message || ''}\n\nWhy: ${r.why || '—'} · delivery: ${r.delivery || '—'}\n${times}`;
          rows.append(row);
        }
        card.append(rows);
      }
      if (c.result) card.append(el('div', 'proposal-status', ui.firstLine(c.result, 140)));
      card.dataset.sig = JSON.stringify([c.state, (c.rows || []).map(r => r.state)]);
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
        // Answered: the open card becomes the card of what was done, in place.
        for (const c of (s && s.cards) || []) {
          const id = 'c:' + c.id; keep.add(id);
          const e = shown.get(id), was = shown.get('p:' + c.id);
          const sig = JSON.stringify([c.state, (c.rows || []).map(r => r.state)]);
          if (e && e.dataset.sig === sig) continue;
          const n = answered(c);
          if (e) e.replaceWith(n); else if (was) { was.replaceWith(n); shown.delete('p:' + c.id); } else list.append(n);
          shown.set(id, n);
        }
        for (const [id, e] of shown) if (!keep.has(id)) { e.remove(); shown.delete(id); }
        // Until the owner has spoken to Overseer (its run exists), home is the composer alone.
        wrap.hidden = !(s && s.run_id) || (messages.length === 0 && open.length === 0 && !((s && s.cards) || []).length);
        list.scrollTop = list.scrollHeight;
      },
      /** A message came back for a proposal card (an error, a state). */
      proposalStatus(id, text) { const e = shown.get('p:' + id); if (e) e.querySelector('.proposal-status').textContent = text; },
      get current() { return session; },
      /** Voice Mode's strip: shown while it is on. */
      voice(v) {
        vstrip.hidden = !(v && v.on);
        if (!v || !v.on) return;
        vstrip.dataset.state = v.state;
        vState.replaceChildren(ui.icon(v.state === 'muted' ? 'mute' : v.state === 'paused' ? 'debug-pause' : 'mic', 'xs'), el('span', null, v.label));
        vState.title = `Voice Mode: ${v.label}${v.reason ? ` (${v.reason})` : ''} · talking to ${v.target}`;
        vHeard.textContent = v.heard ? `“${v.heard}”` : '';
        vHeard.title = v.heard || '';
        vMute.setAttribute('aria-pressed', String(!!v.muted));
        const label = v.muted ? 'Unmute Voice Mode' : 'Mute Voice Mode';
        vMute.setAttribute('aria-label', label); vMute.title = label;
        vMute.replaceChildren(ui.icon(v.muted ? 'mute' : 'mic'));
        if (session && session.run_id) wrap.hidden = false;
      },
    };
  }
  window.OverseerHome = { create };
})();
