// Home (AC-182, AC-227): with no agent selected, the one view for talking to Overseer sits above
// the composer. It is the daemon's one conversation (`overseer.session`): the owner's words, typed
// or spoken, Overseer's replies, the cards (an agent started, a report, a question and its answer, a
// claim, a finding, a check-in that found an agent done, a watch, a share withdrawn) and the
// proposals that wait for a yes. Turning Voice Mode on turns it into the voice view: the mark takes
// the top (voice.js) and the same cards stay below; turning it off returns to the chat. Needs you
// is a small badge with a count that pops out a short list (AC-227). Each request shows its stage,
// in plain words, until it is done, stuck or failed (AC-228); a card opens its agent or its work
// (AC-226).
(function () {
  const ui = window.OverseerUI, el = ui.el;
  const CARD_ICON = { started: 'rocket', report: 'note', ask: 'question', answer: 'comment', claim: 'symbol-folder', done: 'check', trouble: 'warning', while_away: 'history', finding: 'eye', watch: 'eye', watch_ended: 'eye-closed', withdrawn: 'discard', cannot_answer: 'warning', hold: 'debug-pause', release: 'debug-continue', conflict: 'warning', check_in: 'checklist', aside: 'comment-discussion', needs: 'bell' };
  const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user', 'waiting_for_connection', 'waiting_for_memory']);
  const STAGE_ICON = { thinking: 'loading~spin', sending: 'send', waiting: 'question', starting: 'rocket', working: 'sync~spin', stuck: 'bell-dot', done: 'check', failed: 'error', aside: 'comment-discussion' };
  const STUCK_MS = 3 * 60 * 1000;

  // ---------- Plain words (AC-228, AC-245): never an internal token or a raw error (media/plain-words.js).
  const plain = (text, max) => window.OverseerPlain.plain(text, max);
  const clock = ms => ms ? new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : '';
  const elapsed = ms => { const s = Math.max(0, Math.round(ms / 1000)); return s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` : `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`; };

  function create(host, { post, startWith, getState = () => ({ runs: [], attention: [] }), layers }) {
    const wrap = el('section', 'home'); wrap.setAttribute('aria-label', 'Conversation with Overseer'); wrap.id = 'home';
    const head = el('header', 'home-head');
    head.append(ui.mark('sm'), el('span', 'home-title', 'Overseer'));
    // The account Overseer's own run uses (AC-235), named like every agent's.
    const account = el('span', 'home-account ellipsis'); account.id = 'home-account'; account.hidden = true;
    const level = el('span', 'home-level'); level.id = 'home-level';
    const fresh = el('button', 'link', 'Start fresh'); fresh.type = 'button'; fresh.id = 'home-fresh'; fresh.title = 'Archive this conversation and begin a new one (holds, guardrails, areas, conflicts and watches stay)';
    fresh.addEventListener('click', () => post({ type: 'overseerFresh' }));
    // Voice Mode on and off, with its shortcut (AC-217): the view itself turns into the voice view.
    const vToggle = el('button', 'home-voice-toggle'); vToggle.type = 'button'; vToggle.id = 'home-voice-toggle';
    vToggle.setAttribute('aria-pressed', 'false');
    vToggle.append(ui.icon('mic', 'sm'), el('span', 'home-voice-toggle-label', 'Voice'), el('kbd', null, '⌥⌘⇧V'));
    const toggleLabel = on => { const l = on ? 'Turn Voice Mode off' : 'Turn Voice Mode on'; vToggle.setAttribute('aria-label', `${l} (⌥⌘⇧V)`); vToggle.title = `${l} (⌥⌘⇧V)`; };
    toggleLabel(false);
    vToggle.addEventListener('click', () => post({ type: 'voiceToggle' }));
    // Voice Mode's strip (AC-174): the state, the words as they are heard, mute.
    const vstrip = el('div', 'home-voice'); vstrip.id = 'home-voice'; vstrip.hidden = true;
    const vState = el('span', 'home-voice-state'); vState.setAttribute('role', 'status');
    const vHeard = el('span', 'home-voice-heard'); vHeard.setAttribute('aria-live', 'polite');
    const vMute = ui.iconButton('mic', 'Mute Voice Mode', { cls: 'sm', pressed: false }); vMute.id = 'home-voice-mute';
    vMute.addEventListener('click', () => post({ type: 'voiceMute' }));
    vstrip.append(vState, vHeard, vMute);
    // Needs you (AC-227): a badge with a count; a click pops out the short list.
    const needs = el('button', 'home-needs'); needs.type = 'button'; needs.id = 'home-needs'; needs.hidden = true;
    needs.setAttribute('aria-haspopup', 'menu'); needs.setAttribute('aria-expanded', 'false');
    const needsCount = el('span', 'home-needs-count');
    needs.append(ui.icon('bell-dot', 'sm'), needsCount);
    needs.addEventListener('click', () => openNeeds());
    // Beside the agent it started (AC-226): back to the view alone.
    const unaside = ui.iconButton('layout-sidebar-right-off', 'Back to the conversation alone', { cls: 'sm home-unaside' }); unaside.id = 'home-unaside';
    unaside.addEventListener('click', () => post({ type: 'aside', on: false }));
    // The agent followed beside the conversation (AC-257): one click back into its head.
    const backAgent = el('button', 'home-back-agent'); backAgent.type = 'button'; backAgent.id = 'home-back-agent'; backAgent.hidden = true;
    const backLabel = el('span', 'home-back-agent-label');
    backAgent.append(ui.icon('eye', 'sm'), el('span', 'home-back-agent-verb', 'Back to'), backLabel, el('kbd', null, '⌥⌘U'));
    backAgent.addEventListener('click', () => post({ type: 'backToAgent', runId: backAgent.dataset.run }));
    head.append(account, vToggle, vstrip, el('span', 'spacer'), needs, unaside);
    // The conversation's level and Start fresh sit at the bottom right, under the message box
    // (the owner, 2026-09-30), in the composer's foot when it is there.
    const footTools = el('span', 'home-foot-tools'); footTools.append(level, fresh);
    const composerFoot = host.querySelector('.composer-foot');
    if (composerFoot) composerFoot.append(footTools); else head.append(footTools);
    // The voice view's stage: the mark, the words heard and said, and the strip's controls.
    const stage = el('section', 'home-stage'); stage.id = 'voice-stage'; stage.hidden = true; stage.setAttribute('aria-label', 'Voice Mode');
    const voiceStage = window.OverseerVoiceStage && layers ? window.OverseerVoiceStage.create(stage, { post, layers }) : null;
    // What the latest request is doing now, in plain words (AC-228).
    const progress = el('div', 'home-progress'); progress.id = 'home-progress'; progress.setAttribute('role', 'status'); progress.hidden = true;
    const list = el('div', 'home-list'); list.id = 'home-conv'; list.setAttribute('role', 'log'); list.setAttribute('aria-live', 'polite'); list.setAttribute('aria-label', 'Conversation with Overseer');
    wrap.append(head, backAgent, stage, progress, list);
    host.prepend(wrap);
    let session, voiceOn = false, activity = {};
    const shown = new Map(); // message id -> element
    const voiceReqs = new Map(); // V-id -> the voice request (spoken requests' states)
    const stages = new Map(); // owner message id -> the request's stage
    const proposed = new Set(); // owner message ids whose request had a proposal (until its card comes)

    // ---------- The owner's words and Overseer's replies.
    function spokenOf(m) { return m.source === 'owner' && m.surface === 'voice' && /Request (V-\d+): ([\s\S]*)$/.exec(m.text || ''); }
    function row(m) {
      const src = m.source || 'system';
      if (m.card) { const c = card(m); c.dataset.id = m.id; c.dataset.ts = m.ts; return c; }
      const r = el('div', `home-msg from-${src}`); r.dataset.id = m.id; r.dataset.ts = m.ts;
      const who = el('div', 'home-from');
      // A spoken request (Voice Mode): the owner's words, not the notes the daemon adds for Overseer.
      const spoken = spokenOf(m);
      if (src === 'overseer') who.append(ui.mark('sm'), el('span', null, 'Overseer'));
      else if (spoken) { const mic = ui.icon('mic', 'xs'); who.append(mic, el('span', null, 'You, by voice')); r.classList.add('spoken'); r.dataset.request = spoken[1]; }
      else if (src === 'owner') who.append(ui.icon('account', 'xs'), el('span', null, 'You'));
      else who.append(ui.icon('info', 'xs'), el('span', null, { agent: 'An agent', system: 'Overseer' }[src] || 'Note'));
      let words = spoken ? spoken[2].trim() : m.text || '';
      // An agent named with `@` reaches Overseer with its id; the owner reads the name alone (AC-245).
      if (src === 'owner') words = words.replace(/\s\((?:r|p|sh|w)-[0-9a-f]{6,}\)/g, '');
      // Overseer's own notes about a proposal that could not be made: the reason, in plain words.
      const failed = src === 'overseer' && /^\(The proposal could not be (?:made|read)[:,]?\s*([\s\S]*?)\)$/.exec(words);
      if (failed) words = `I couldn't do that${failed[1] ? ': ' + plain(failed[1]) : '.'}`;
      const text = el('div', 'home-text', words);
      // Overseer's replies are Markdown, as in an agent's chat (lists, code, a quoted diff).
      if (src === 'overseer' && !failed && window.OverseerMarkdown) { text.classList.add('md'); window.OverseerMarkdown.render(text, window.OverseerPlain.states(words), { post }); }
      r.append(who, text);
      if (src === 'owner') {
        const st = el('button', 'req-stage'); st.type = 'button'; st.hidden = true;
        st.addEventListener('click', () => { const s = stages.get(m.id); if (s && s.run) post({ type: 'openAgent', runId: s.run, work: s.stage === 'done' }); });
        r.append(st);
      }
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
      head.append(ui.icon(CARD_ICON[kind] || 'info', 'sm'), el('span', 'card-text', kind === 'cannot_answer' ? plain(m.text, 220) : m.text || kind));
      r.append(head);
      const detail = [];
      if (kind === 'report') { if (c.needs) detail.push(`Needs: ${c.needs}`); if (c.blocked) detail.push(`Blocked by: ${c.blocked}`); if (c.changed && c.changed.length) detail.push(`Changed: ${c.changed.join(', ')}`); }
      if (kind === 'ask') detail.push(c.answer ? `Answer: ${c.answer}` : 'Waiting for an answer');
      if (kind === 'answer') detail.push(`Question: ${c.question}`);
      if (kind === 'done') { if (c.done) detail.push(c.done); if (c.left_out) detail.push(`Left out: ${c.left_out}`); }
      if (kind === 'finding') { detail.push(`${c.result}${c.held ? ' · held at once' : ''}${c.snapshot ? ' · snapshot ' + String(c.snapshot).slice(0, 8) : ''}`); }
      if (kind === 'started') {
        // Which account the new agent runs on (AC-235).
        const run = (getState().runs || []).find(x => x.id === c.agent);
        const p = run && run.profile_id && (getState().profiles || []).find(x => x.id === run.profile_id);
        if (p) detail.push((p.account && p.account.short) || p.name);
        if (c.prompt) detail.push(ui.firstLine(c.prompt, 160));
      }
      if (kind === 'cannot_answer' && c.reason) detail.push(plain(c.reason));
      if (detail.length) r.append(el('div', 'card-detail', detail.join(' · ')));
      const agent = c.agent || c.run_id;
      if (agent) {
        // A card opens its agent (AC-226).
        r.classList.add('opens'); r.dataset.run = agent; r.tabIndex = 0; r.title = 'Open this agent';
        const open = () => post({ type: 'openAgent', runId: agent });
        r.addEventListener('click', e => { if (!e.target.closest('button')) open(); });
        r.addEventListener('keydown', e => { if ((e.key === 'Enter' || e.key === ' ') && e.target === r) { e.preventDefault(); open(); } });
      }
      if (kind === 'started' && c.agent) {
        // Meant for Overseer after all: stop the agent just started and ask instead.
        const b = el('button', 'link', 'Ask Overseer instead'); b.type = 'button'; b.dataset.action = 'ask-overseer-instead';
        b.addEventListener('click', () => { b.disabled = true; post({ type: 'overseerUndoStart', runId: c.agent, text: c.prompt || '' }); });
        r.append(b);
      }
      return r;
    }

    function proposal(p) {
      const card = el('div', 'proposal'); card.setAttribute('role', 'group'); card.setAttribute('aria-label', 'Overseer proposes'); card.dataset.id = p.id; card.dataset.ts = p.ts;
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
      if (p.state === 'settling') { status.textContent = 'Going out in a moment unless you cancel it'; row.hidden = true; }
      else if (p.state && p.state !== 'open') { status.textContent = plain(p.result || p.state); row.hidden = true; card.classList.add('answered'); }
      return card;
    }

    // An answered proposal (AC-185): what Overseer did, with one row per agent: why it was chosen,
    // the delivery, the state and when it got there; the whole text sent is the row's tooltip. A
    // row opens its agent (AC-226).
    const STATE = { yes: 'Done', done: 'Done', no: 'Declined', cancelled: 'Cancelled', stale: 'Not done', not_done: 'Not done', failed: 'Failed', refused: 'Refused' };
    // Why an agent was chosen and how the words reached it, in words (AC-245).
    const WHY = { named: 'you named it' };
    const DELIVERY = { add: 'added to its work', advisory: 'for its information', start: 'a new agent', stop: 'stop', allow: 'allowed', deny: 'denied', guardrail: 'a guardrail', auto: 'as it fits' };
    const ROW = { held: 'held', sent: 'sent', delivered: 'delivered', picked_up: 'picked up', answered: 'answered', failed: 'failed', cancelled: 'cancelled', not_sent: 'not sent' };
    function answered(c) {
      const card = el('div', 'proposal answered done-card'); card.setAttribute('role', 'group'); card.setAttribute('aria-label', 'What Overseer did'); card.dataset.id = c.id; card.dataset.state = c.state; card.dataset.ts = c.ts;
      const head = el('div', 'proposal-head'); head.append(ui.mark('sm'), el('span', null, STATE[c.state] || plain(c.state)));
      const by = window.OverseerPlain.answeredBy(c.answered_by, c.surface);
      if (by) head.append(el('span', 'card-by', by));
      const list = el('ul', 'proposal-list');
      for (const line of c.lines || []) list.append(el('li', null, line));
      card.append(head, list);
      if ((c.rows || []).length) {
        const rows = el('div', 'card-rows'); rows.setAttribute('role', 'list');
        for (const r of c.rows) {
          const row = el('div', 'card-row'); row.setAttribute('role', 'listitem'); row.dataset.state = r.state; row.dataset.run = r.run_id || '';
          const at = r.answered_ms || r.picked_ms || r.delivered_ms || r.sent_ms || r.held_ms;
          const times = [['held', r.held_ms], ['sent', r.sent_ms], ['delivered', r.delivered_ms], ['picked up', r.picked_ms], ['answered', r.answered_ms]].filter(x => x[1]).map(([k, v]) => `${k} ${clock(v)}`).join(' · ');
          const who = el('button', 'card-row-agent link', r.title || (r.action === 'start' ? 'New agent' : 'Agent')); who.type = 'button';
          who.disabled = !r.run_id; who.title = r.run_id ? 'Open this agent' : 'Not started';
          who.addEventListener('click', () => post({ type: 'openAgent', runId: r.run_id }));
          row.append(who, el('span', 'card-row-meta', [WHY[r.why] || r.why, DELIVERY[r.delivery] || r.delivery].filter(Boolean).join(' · ')), el('span', `card-row-state state-${r.state}`, ROW[r.state] || plain(r.state)), el('span', 'card-row-time', clock(at)));
          row.title = `${(r.message || '').replace(/\s\((?:r|p|sh|w)-[0-9a-f]{6,}\)/g, '')}\n\nWhy: ${WHY[r.why] || r.why || '—'} · ${DELIVERY[r.delivery] || r.delivery || '—'}\n${times}`;
          rows.append(row);
        }
        card.append(rows);
      }
      if (c.result) card.append(el('div', 'proposal-status', plain(c.result, 140)));
      card.dataset.sig = JSON.stringify([c.state, (c.rows || []).map(r => r.state)]);
      return card;
    }

    // A spoken request that never reached the conversation (a built-in phrase, a stop, a
    // permission answered, a message straight to the agent spoken to): a row of its own, by time.
    function voiceRow(r) {
      const e = el('div', 'home-msg from-owner spoken voice-only'); e.dataset.id = 'v:' + r.id; e.dataset.ts = r.ts; e.dataset.request = r.id;
      const who = el('div', 'home-from'); who.append(ui.icon('mic', 'xs'), el('span', null, 'You, by voice'));
      const st = el('button', 'req-stage'); st.type = 'button'; st.hidden = true;
      st.addEventListener('click', () => { const s = stages.get('v:' + r.id); if (s && s.run) post({ type: 'openAgent', runId: s.run, work: s.stage === 'done' }); });
      e.append(who, el('div', 'home-text', r.words || ''), st);
      return e;
    }

    /** Keeps the list in time order: a row is placed before the first later one. */
    function place(e) {
      const ts = Number(e.dataset.ts || 0);
      const later = ts && [...list.children].find(x => Number(x.dataset.ts || 0) > ts);
      if (later) list.insertBefore(e, later); else list.append(e);
    }

    // ---------- Stages (AC-228): thinking, going out or waiting for a yes, starting the agent,
    // working (live activity and elapsed time), then done, stuck or failed.
    const runOf = id => (getState().runs || []).find(r => r.id === id);
    function agentStage(row, card) {
      const run = row.run_id && runOf(row.run_id);
      const title = (run && run.title) || row.title || 'the agent';
      if (row.state === 'failed' || row.state === 'not_sent') return { stage: 'failed', text: row.action === 'start' ? `Could not start ${title}${row.message && /Not started: ([^\n]*)/.test(row.message) ? ': ' + plain(/Not started: ([^\n]*)/.exec(row.message)[1]) : ''}` : `Not sent to ${title}`, run: row.run_id };
      if (row.state === 'cancelled') return { stage: 'done', text: `Cancelled for ${title}`, run: row.run_id };
      if (!run) return row.action === 'start' ? { stage: 'starting', text: `Starting ${title}`, run: row.run_id } : { stage: 'done', text: `Sent to ${title}`, run: row.run_id };
      const act = activity[run.id];
      const since = row.action === 'start' ? run.created_ms : (row.delivered_ms || row.sent_ms || row.held_ms || card.answered_ms || run.created_ms);
      switch (run.status) {
        case 'queued': case 'starting': return { stage: 'starting', text: row.action === 'start' ? `Starting ${title}` : `${title} is starting`, run: run.id };
        case 'running': {
          if (row.action !== 'start' && row.state === 'held') return { stage: 'working', text: `${title} gets it when its turn ends`, run: run.id, since };
          const quiet = act && act.at ? Date.now() - act.at : Date.now() - since;
          if (quiet > STUCK_MS) return { stage: 'stuck', text: `${title} has shown no activity for ${elapsed(quiet)}`, run: run.id, since };
          return { stage: 'working', text: `${title} is working`, detail: act && act.text ? act.text : '', run: run.id, since };
        }
        case 'waiting_for_user': return { stage: 'stuck', text: `${title} needs you${run.attention && run.attention.tool ? `: it wants to use ${run.attention.tool}` : ''}`, run: run.id };
        case 'waiting_for_connection': return { stage: 'stuck', text: `${title} waits for a connection`, run: run.id };
        case 'waiting_for_memory': return { stage: 'stuck', text: `${title} waits for memory`, run: run.id };
        case 'completed': return { stage: 'done', text: row.action === 'start' ? `${title} finished` : row.state === 'answered' || row.state === 'picked_up' ? `${title} is done with it` : `${title} finished`, run: run.id };
        // The daemon's plain reason follows the agent's name ("reached its account's usage limit", AC-239).
        case 'failed': case 'disconnected': return { stage: 'failed', text: run.plain_reason ? `${title} ${run.plain_reason.charAt(0).toLowerCase()}${run.plain_reason.slice(1)}` : `${title} failed${run.exit_reason ? ': ' + plain(run.exit_reason, 90) : ''}`, run: run.id };
        case 'interrupted': return { stage: 'failed', text: `${title} was stopped`, run: run.id };
        default: return { stage: 'done', text: `${title}: ${ui.statusText(run.status)}`, run: run.id };
      }
    }
    const ORDER = ['failed', 'stuck', 'starting', 'working', 'done'];
    function cardStage(c) {
      if (!c) return null;
      if (c.state === 'no') return { stage: 'done', text: 'Declined: nothing was done' };
      if (c.state === 'cancelled') return { stage: 'done', text: 'Cancelled: nothing was sent' };
      if (c.state === 'stale' || c.state === 'not_done') return { stage: 'failed', text: plain(c.result) || 'Not done' };
      if (c.state === 'failed' || c.state === 'refused') return { stage: 'failed', text: plain(c.result) || 'It did not work' };
      const rows = (c.rows || []).filter(r => r.action !== 'swarm');
      if (!rows.length) return { stage: /did not work/.test(plain(c.result)) ? 'failed' : 'done', text: plain(c.result) || 'Done' };
      const each = rows.map(r => agentStage(r, c));
      each.sort((a, b) => ORDER.indexOf(a.stage) - ORDER.indexOf(b.stage));
      const s = each[0];
      if (rows.length > 1 && s.stage === 'done') return { ...s, text: `Done: ${rows.length} agents` };
      return s;
    }
    const VOICE_STAGE = { taken: ['thinking', 'Overseer is thinking'], thinking: ['thinking', 'Overseer is thinking'], settling: ['sending', 'Going out in a moment'], waiting: ['waiting', 'Waits for your yes'], waiting_turn: ['thinking', 'Waits for the requests before it'], sent: ['done', 'Sent'], partly_sent: ['failed', 'Partly sent: the card says what'], cancelled: ['done', 'Cancelled: nothing was sent'], corrected: ['done', 'Corrected by what you said next'], superseded: ['done', 'Replaced by your correction'], joined: ['done', 'Joined with what you said next'], not_sent: ['failed', 'Not sent'], answered: ['done', 'Answered'], done: ['done', 'Done'], not_for_overseer: ['aside', 'Not meant for Overseer: kept as context'] };
    function stageOf(owner, group, latest) {
      const spoken = spokenOf(owner);
      const vr = spoken ? voiceReqs.get(spoken[1]) : owner.voice;
      if (vr && ['not_for_overseer', 'cancelled', 'not_sent', 'corrected', 'superseded', 'joined'].includes(vr.state)) {
        const [stage, text] = VOICE_STAGE[vr.state];
        return { stage, text: vr.state === 'not_sent' && vr.done ? plain(vr.done) : text };
      }
      const byProposal = vr && vr.proposal && ((session && session.cards) || []).find(c => c.id === vr.proposal);
      const c = byProposal || group.cards[group.cards.length - 1];
      if (c) return cardStage(c);
      const p = (vr && vr.proposal && ((session && session.proposals) || []).find(x => x.id === vr.proposal)) || group.proposals[group.proposals.length - 1];
      if (p) return p.state === 'settling' ? { stage: 'sending', text: 'Going out in a moment unless you cancel it' } : { stage: 'waiting', text: 'Waits for your yes' };
      // Answered a moment ago and being carried out: not a card yet.
      if (proposed.has(owner.id)) return { stage: 'sending', text: 'Going ahead' };
      const turn = session && ACTIVE.has(session.run_status);
      const failedReply = group.replies.find(m => m.card && m.card.kind === 'cannot_answer');
      if (failedReply) return { stage: 'failed', text: `Overseer couldn't answer${failedReply.card.reason ? ': ' + plain(failedReply.card.reason, 100) : ''}` };
      if (group.replies.some(m => m.card && m.card.kind === 'aside')) return { stage: 'aside', text: VOICE_STAGE.not_for_overseer[1] };
      if (latest && (turn || (vr && ['taken', 'thinking', 'waiting_turn'].includes(vr.state)))) return { stage: 'thinking', text: group.replies.length ? 'Overseer is still thinking' : 'Overseer is thinking' };
      if (vr && VOICE_STAGE[vr.state]) { const [stage, text] = VOICE_STAGE[vr.state]; return { stage, text: vr.done ? plain(vr.done) : text }; }
      if (group.replies.length) return { stage: 'done', text: 'Answered' };
      if (latest && Date.now() - owner.ts < 15000) return { stage: 'thinking', text: 'Overseer is thinking' };
      return null;
    }
    function showStage(e, s) {
      const st = e && e.querySelector('.req-stage');
      if (!st) return;
      if (!s) { st.hidden = true; return; }
      const text = s.stage === 'working' ? `${s.text} · ${elapsed(Date.now() - (s.since || Date.now()))}${s.detail ? ' · ' + s.detail : ''}` : s.text;
      const sig = s.stage + '|' + text;
      if (st.dataset.sig === sig) return;
      st.dataset.sig = sig; st.dataset.stage = s.stage; st.hidden = false;
      st.replaceChildren(ui.icon(STAGE_ICON[s.stage] || 'info', 'xs'), el('span', 'req-stage-text', text));
      st.disabled = !s.run;
      st.title = s.run ? (s.stage === 'done' ? 'Open its work' : 'Open the agent') : text;
      st.setAttribute('aria-label', text);
    }
    let stageTimer;
    function renderStages() {
      if (!session) return;
      const messages = session.messages || [];
      const owners = messages.filter(m => m.source === 'owner');
      // Spoken requests with no message in the conversation, placed by time.
      const inConversation = new Set(owners.map(m => (spokenOf(m) || [])[1]).filter(Boolean));
      const voiceOnly = [...voiceReqs.values()].filter(r => !inConversation.has(r.id) && Date.now() - r.ts < 6 * 3600000).map(r => ({ id: 'v:' + r.id, source: 'owner', surface: 'voice', ts: r.ts, voice: r }));
      const all = [...owners, ...voiceOnly].sort((a, b) => a.ts - b.ts);
      const groups = new Map(all.map(o => [o.id, { replies: [], proposals: [], cards: [] }]));
      const ownerAt = ts => { let o; for (const x of all) { if (x.ts <= ts && !x.voice) o = x; } return o; };
      for (const m of messages) if (m.source !== 'owner') { const o = ownerAt(m.ts); if (o) groups.get(o.id).replies.push(m); }
      for (const p of session.proposals || []) { const o = ownerAt(p.ts); if (o) groups.get(o.id).proposals.push(p); }
      for (const c of session.cards || []) { const o = ownerAt(c.ts); if (o) groups.get(o.id).cards.push(c); }
      for (const [id, g] of groups) { if (g.proposals.length) proposed.add(id); if (g.cards.length) proposed.delete(id); }
      let any = false, last = null;
      all.forEach((o, i) => {
        const s = stageOf(o, groups.get(o.id), i === all.length - 1);
        stages.set(o.id, s);
        if (o.voice && !shown.has(o.id)) { const e = voiceRow(o.voice); shown.set(o.id, e); place(e); }
        showStage(shown.get(o.id), s);
        if (s && (s.stage === 'working' || s.stage === 'thinking' || s.stage === 'starting' || s.stage === 'sending')) any = true;
        if (s) last = s;
      });
      // The view says what the latest request is doing (AC-228).
      if (last && (Date.now() - (all[all.length - 1] || {}).ts < 30 * 60000 || last.stage !== 'done')) {
        const text = last.stage === 'working' ? `${last.text} · ${elapsed(Date.now() - (last.since || Date.now()))}${last.detail ? ' · ' + last.detail : ''}` : last.text;
        progress.hidden = false; progress.dataset.stage = last.stage;
        progress.replaceChildren(ui.icon(STAGE_ICON[last.stage] || 'info', 'sm'), el('span', null, text));
      } else progress.hidden = true;
      clearTimeout(stageTimer);
      if (any) stageTimer = setTimeout(renderStages, 1000);
      refreshVisibility();
    }

    function refreshVisibility() {
      const messages = (session && session.messages) || [];
      const open = ((session && session.proposals) || []).filter(p => p.state === 'open' || p.state === 'settling');
      // Until the owner has spoken to Overseer (its run exists), home is the composer alone, with
      // only the Voice button and Needs you in its head; Voice Mode on shows the conversation.
      // What happened while the owner was away leads even before the first word (AC-253).
      const talked = !!(session && session.run_id) || voiceOn || voiceReqs.size > 0 || messages.some(m => m.card && m.card.kind === 'while_away');
      const empty = !talked || (messages.length === 0 && open.length === 0 && !((session && session.cards) || []).length && !voiceReqs.size);
      // The head (voice on and off, Needs you) is always there; the conversation once there is one.
      list.hidden = empty;
      wrap.dataset.empty = empty ? '1' : '';
      // With a conversation the view is the conversation, the composer at its foot (AC-227).
      document.body.dataset.conversation = empty ? '' : '1';
    }

    function renderAccount() {
      const a = getState().overseerAccount;
      account.hidden = !a;
      account.textContent = a ? a.short : '';
      if (a) { account.title = `Overseer runs on ${a.label}`; account.setAttribute('aria-label', account.title); }
    }

    // ---------- Needs you (AC-227): the badge, its short list, a click focuses the agent.
    function needsList() { return (getState().attention || []).filter(a => a.run_id); }
    function renderNeeds() {
      const items = needsList();
      needs.hidden = !items.length;
      needsCount.textContent = String(items.length);
      const label = `Needs you: ${items.length}`;
      needs.setAttribute('aria-label', label); needs.title = `${label}. Click for the list.`;
    }
    function openNeeds() {
      const runs = getState().runs || [];
      const items = needsList().map(a => {
        const run = runs.find(r => r.id === a.run_id);
        const title = a.overseer ? 'Overseer' : (run && run.title) || 'An agent';
        return { label: title, hint: a.label, icon: a.overseer ? 'comment-discussion' : a.label === 'Approve' ? 'shield' : a.label === 'Failed' ? 'error' : a.label === 'Review' ? 'diff' : 'bell', title: a.detail || a.label,
          id: 'needs-' + a.run_id, run: () => { if (a.overseer) { const p = list.querySelector('.proposal:not(.answered)'); if (p) p.scrollIntoView({ block: 'center' }); } else post({ type: 'openAgent', runId: a.run_id, from: 'needs' }); } };
      });
      if (!items.length) return;
      ui.menu(needs, [{ head: 'Needs you' }, ...items], { align: 'end', label: 'Needs you' });
    }

    return {
      /** The daemon's session: redrawn from its messages and open proposals (cheap: ids are stable). */
      session(s) {
        session = s;
        const messages = (s && s.messages) || [];
        // Before the owner's first word only the away line shows, not every agent's start (AC-253).
        const before = !!s && !s.run_id && !voiceOn;
        const open = ((s && s.proposals) || []).filter(p => p.state === 'open' || p.state === 'settling');
        level.textContent = s && s.level ? { ask_first: 'Ask first', steer: 'Steer', auto: 'Auto' }[s.level] || s.level : '';
        const keep = new Set();
        let away = null;
        for (const m of messages) {
          keep.add(m.id);
          let e = shown.get(m.id);
          if (!e) { e = row(m); shown.set(m.id, e); place(e); if (m.card && m.card.kind === 'while_away') away = e; }
          else if (m.card && m.card.kind === 'ask' && e.dataset.answer !== String(m.card.answer || '')) { const n = row(m); e.replaceWith(n); shown.set(m.id, n); e = n; }
          if (m.card && m.card.kind === 'ask') e.dataset.answer = String(m.card.answer || '');
          e.hidden = before && !(m.card && m.card.kind === 'while_away');
        }
        for (const p of open) {
          const id = 'p:' + p.id; keep.add(id);
          const e = shown.get(id);
          if (!e || e.dataset.pstate !== p.state) { const n = proposal(p); n.dataset.pstate = p.state; if (e) e.replaceWith(n); else place(n); shown.set(id, n); }
        }
        // Answered: the open card becomes the card of what was done, in place.
        for (const c of (s && s.cards) || []) {
          const id = 'c:' + c.id; keep.add(id);
          const e = shown.get(id), was = shown.get('p:' + c.id);
          const sig = JSON.stringify([c.state, (c.rows || []).map(r => r.state)]);
          if (e && e.dataset.sig === sig) continue;
          const n = answered(c);
          if (e) e.replaceWith(n); else if (was) { was.replaceWith(n); shown.delete('p:' + c.id); } else place(n);
          shown.set(id, n);
        }
        for (const [id, e] of shown) if (!keep.has(id) && !id.startsWith('v:')) { e.remove(); shown.delete(id); }
        renderStages();
        list.scrollTop = list.scrollHeight;
        // What happened while the owner was away leads (AC-253): in view once the layout settles.
        if (away) { const lead = away; requestAnimationFrame(() => lead.scrollIntoView({ block: 'end' })); setTimeout(() => lead.isConnected && lead.scrollIntoView({ block: 'end' }), 700); }
      },
      /** A message came back for a proposal card (an error, a state). */
      proposalStatus(id, text) { const e = shown.get('p:' + id); if (e) e.querySelector('.proposal-status').textContent = plain(text); },
      get current() { return session; },
      /** Voice Mode's summary: on, the strip's state, the words as they are heard. */
      voice(v) {
        const on = !!(v && v.on);
        if (on !== voiceOn) {
          voiceOn = on;
          document.body.dataset.voice = on ? 'on' : 'off';
          if (on && voiceStage) voiceStage.wake();
          // Before the first word the list held only the away line: redraw it whole or back again.
          if (session && !session.run_id) this.session(session); else refreshVisibility();
        }
        // Stopped by failures: the stage stays to say why, with Turn on.
        stage.hidden = !(on || (v && v.stopped));
        vToggle.setAttribute('aria-pressed', String(on));
        vToggle.classList.toggle('on', on);
        toggleLabel(on);
        vstrip.hidden = !on;
        if (!on) return;
        vstrip.dataset.state = v.state;
        vState.replaceChildren(ui.icon(v.state === 'muted' ? 'mute' : v.state === 'paused' ? 'debug-pause' : 'mic', 'xs'), el('span', null, v.label));
        vState.title = `Voice Mode: ${v.label}${v.reason ? ` (${v.reason})` : ''} · talking to ${v.target}`;
        vHeard.textContent = v.heard ? `“${v.heard}”` : '';
        vHeard.title = v.heard || '';
        vMute.setAttribute('aria-pressed', String(!!v.muted));
        const label = v.muted ? 'Unmute Voice Mode' : 'Mute Voice Mode';
        vMute.setAttribute('aria-label', label); vMute.title = label;
        vMute.replaceChildren(ui.icon(v.muted ? 'mute' : 'mic'));
      },
      /** The voice stage's own messages (the mark's levels and state, the words heard and said). */
      voiceView(m) {
        if (voiceStage) voiceStage.receive(m);
        if (m.type === 'requests') { for (const r of m.list || []) voiceReqs.set(r.id, r); renderStages(); }
        else if (m.type === 'live' && m.msg && m.msg.kind === 'request' && m.msg.request) { voiceReqs.set(m.msg.request.id, m.msg.request); renderStages(); }
        const open = [...voiceReqs.values()].sort((a, b) => b.ts - a.ts).find(r => ['settling', 'thinking', 'taken'].includes(r.state));
        if (voiceStage) voiceStage.setOpen(open);
      },
      /** The agent whose head is shown beside the conversation (AC-257), or none. */
      headAgent(agent) {
        backAgent.hidden = !(agent && agent.runId);
        if (!agent || !agent.runId) return;
        backAgent.dataset.run = agent.runId;
        backLabel.textContent = agent.title || 'the agent';
        const label = `Back to ${agent.title || 'the agent'} (⌥⌘U)`;
        backAgent.title = label; backAgent.setAttribute('aria-label', label);
      },
      /** What each agent is doing right now (its last tool or words), for the working stage. */
      activity(map) { activity = map || {}; renderStages(); },
      /** The daemon's state changed (runs, Needs you). */
      state() { renderNeeds(); renderStages(); renderAccount(); },
      plain,
    };
  }
  window.OverseerHome = { create, plain };
})();
