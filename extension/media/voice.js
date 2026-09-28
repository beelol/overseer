// The voice view (Voice Mode, Gate R): the Overseer mark in the middle, moved by the daemon's
// levels (AC-177); the words as they are heard; the strip (state, who is spoken to, cancel, mute);
// and the spoken requests with what was sent to each agent (AC-169, AC-174). The daemon owns all of
// it: this page only draws what arrives on the live channel.
(function () {
  'use strict';
  const vscode = acquireVsCodeApi();
  const $ = id => document.getElementById(id);
  const main = document.querySelector('main.voice');
  const layerUris = JSON.parse(main.dataset.layers);
  const LABEL = { off: 'Off', starting: 'Starting', listening: 'Listening', hearing: 'Hearing you', thinking: 'Thinking', speaking: 'Speaking', muted: 'Muted', paused: 'Paused for a call', failed: 'Stopped' };
  const MARK_STATE = { off: 'muted', starting: 'listening', listening: 'listening', hearing: 'hearing', thinking: 'thinking', speaking: 'speaking', muted: 'muted', paused: 'paused', failed: 'muted' };
  const STATE_LABEL = { taken: 'Taken', thinking: 'Working on it', settling: 'Going out', waiting: 'Waits for your yes', sent: 'Sent', partly_sent: 'Partly sent', cancelled: 'Cancelled', corrected: 'Corrected', joined: 'Joined', not_sent: 'Not sent', answered: 'Answered', done: 'Done', not_for_overseer: 'Not for Overseer' };
  const SIGN = {
    mute: '<i class="codicon codicon-mute" aria-hidden="true"></i>',
    pause: '<i class="codicon codicon-debug-pause" aria-hidden="true"></i>',
  };
  let voice = null;
  let mark = null;
  let heardFinal = '';
  const requests = new Map();
  const cards = new Map();
  const reducedQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
  let reducedSetting = false;

  function load(src) {
    return new Promise((ok, fail) => { const i = new Image(); i.onload = () => ok(i); i.onerror = fail; i.src = src; });
  }
  Promise.all(['core', 'swooshes', 'star', 'flat'].map(k => load(layerUris[k]).then(img => [k, img]))).then(pairs => {
    const layers = Object.fromEntries(pairs);
    mark = window.OverseerVoiceMark.mount($('voice-canvas'), layers, { reduced: reducedQuery.matches || reducedSetting });
    window.__voice.mark = mark;
    applyState();
  });
  reducedQuery.addEventListener('change', () => mark && mark.setReduced(reducedQuery.matches || reducedSetting));
  window.__voice = { mark: null, messages: 0 };

  function applyState() {
    const state = voice ? voice.state : 'off';
    $('voice-state').textContent = LABEL[state] || state;
    $('voice-state').dataset.state = state;
    const reason = voice && voice.reason;
    $('voice-state').title = reason ? `${LABEL[state]}: ${reason}` : LABEL[state] || state;
    const off = !voice || !voice.enabled;
    $('voice-off').hidden = !off;
    $('voice-off-reason').textContent = reason && off ? reason : '';
    $('voice-mute').hidden = off;
    const muted = voice && voice.muted;
    $('voice-mute').setAttribute('aria-pressed', muted ? 'true' : 'false');
    $('voice-mute').title = muted ? 'Unmute: open the microphone' : 'Mute: close the microphone';
    $('voice-mute').setAttribute('aria-label', $('voice-mute').title);
    $('voice-mute').innerHTML = `<i class="codicon codicon-${muted ? 'mute' : 'mic'}" aria-hidden="true"></i>`;
    const m = MARK_STATE[state] || 'listening';
    if (mark) mark.setState(m);
    const sign = m === 'muted' && state !== 'off' && state !== 'failed' ? 'mute' : m === 'paused' ? 'pause' : null;
    $('voice-sign').hidden = !sign;
    $('voice-sign').innerHTML = sign ? SIGN[sign] : '';
    main.dataset.state = state;
    $('voice-target').textContent = voice && voice.target_title ? voice.target_title : 'Overseer';
    $('voice-target').title = 'Who you are talking to (Voice Mode: Talk To…)';
  }

  function renderRequests() {
    const list = [...requests.values()].sort((a, b) => b.ts - a.ts).slice(0, 12);
    const box = $('voice-requests');
    box.hidden = list.length === 0;
    const open = list.find(r => r.state === 'settling' || r.state === 'thinking' || r.state === 'taken');
    $('voice-cancel').hidden = !open;
    $('voice-cancel').dataset.id = open ? open.id : '';
    box.replaceChildren(...list.map(r => {
      const el = document.createElement('article');
      el.className = `vreq state-${r.state}`;
      el.dataset.id = r.id;
      const head = document.createElement('header');
      const id = document.createElement('span'); id.className = 'vreq-id'; id.textContent = r.id;
      const chip = document.createElement('span'); chip.className = 'vreq-state'; chip.textContent = STATE_LABEL[r.state] || r.state;
      const when = document.createElement('time'); when.className = 'vreq-time'; when.textContent = new Date(r.ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      head.append(id, chip, when);
      const words = document.createElement('p'); words.className = 'vreq-words'; words.textContent = `“${r.words}”`;
      el.append(head, words);
      const card = r.proposal && cards.get(r.proposal);
      if (card && card.rows && card.rows.length) {
        const rows = document.createElement('ul'); rows.className = 'vreq-rows';
        for (const row of card.rows) {
          const li = document.createElement('li');
          const who = document.createElement('button'); who.className = 'link vreq-agent'; who.textContent = row.title || row.run_id; who.title = 'Open this agent';
          who.addEventListener('click', () => vscode.postMessage({ type: 'open', run: row.run_id }));
          const st = document.createElement('span'); st.className = 'vreq-row-state'; st.textContent = row.state.replace('_', ' ');
          const details = document.createElement('details');
          const summary = document.createElement('summary'); summary.textContent = (row.message || '').split('\n').find(l => l.startsWith('For you:')) || (row.message || '').split('\n')[0];
          const full = document.createElement('pre'); full.textContent = row.message || '';
          details.append(summary, full);
          li.append(who, st, details);
          rows.append(li);
        }
        el.append(rows);
      } else if (r.done || r.answer) {
        const note = document.createElement('p'); note.className = 'vreq-note'; note.textContent = r.done || r.answer;
        el.append(note);
      }
      return el;
    }));
  }

  window.addEventListener('message', ({ data: m }) => {
    if (!m || typeof m !== 'object') return;
    window.__voice.messages += 1;
    if (m.type === 'snapshot') {
      voice = m.voice;
      reducedSetting = !!m.reducedMotion;
      if (mark) mark.setReduced(reducedQuery.matches || reducedSetting);
      $('voice-meter').hidden = !(reducedQuery.matches || reducedSetting);
      applyState();
    } else if (m.type === 'requests') {
      for (const r of m.list) requests.set(r.id, r);
      renderRequests();
    } else if (m.type === 'card') {
      cards.set(m.card.id, m.card);
      renderRequests();
    } else if (m.type === 'live') {
      const v = m.msg;
      if (v.kind === 'state' && voice) { voice.state = v.state; voice.reason = v.reason; applyState(); }
      else if (v.kind === 'level') {
        if (mark) mark.setLevel(v.source, v.value);
        $('voice-meter').firstElementChild.style.width = `${Math.round(v.value * 100)}%`;
        const log = window.__voice.levels || (window.__voice.levels = []);
        log.push({ t: performance.now(), source: v.source, value: v.value });
        if (log.length > 2000) log.shift();
      }
      else if (v.kind === 'heard') {
        const el = $('voice-heard');
        el.textContent = v.text;
        el.classList.remove('aside');
        el.classList.toggle('final', !!v.final);
        clearTimeout(window.__voice.fade);
        // The words stay while they matter, then give way (the request's card keeps them).
        if (v.final) { heardFinal = v.text; window.__voice.fade = setTimeout(() => { if (el.textContent === heardFinal) el.textContent = ''; }, 6000); }
      } else if (v.kind === 'not_meant') {
        const el = $('voice-heard'); el.textContent = v.text; el.classList.add('aside'); el.title = 'Not meant for Overseer: kept in memory only';
      } else if (v.kind === 'request' && v.request) {
        requests.set(v.request.id, v.request);
        renderRequests();
      } else if (v.kind === 'target' && voice) { voice.target = v.target; voice.target_title = v.target_title || (v.target === 'overseer' ? 'Overseer' : voice.target_title); applyState(); }
    }
  });
  $('voice-mute').addEventListener('click', () => vscode.postMessage({ type: 'mute' }));
  $('voice-on').addEventListener('click', () => vscode.postMessage({ type: 'toggle' }));
  $('voice-target').addEventListener('click', () => vscode.postMessage({ type: 'target' }));
  $('voice-cancel').addEventListener('click', e => vscode.postMessage({ type: 'cancel', id: e.currentTarget.dataset.id }));
  vscode.postMessage({ type: 'ready' });
  void heardFinal;
})();
