// Voice Mode's stage (Gate R, AC-227): the Overseer mark, moved by the daemon's levels (AC-177);
// the words as they are heard; the strip (state, who is spoken to, yes, no, cancel, mute). Since
// AC-227 it is not a view of its own: turning Voice Mode on turns the conversation with Overseer
// (home) into the voice view, with this stage on top and the same cards below it, and turning it
// off returns to the chat. The daemon owns all of it: this only draws what the live channel says.
(function () {
  'use strict';
  const LABEL = { off: 'Off', starting: 'Starting', listening: 'Listening', hearing: 'Hearing you', thinking: 'Thinking', speaking: 'Speaking', muted: 'Muted', paused: 'Paused for a call', failed: 'Stopped' };
  const MARK_STATE = { off: 'muted', starting: 'listening', listening: 'listening', hearing: 'hearing', thinking: 'thinking', speaking: 'speaking', muted: 'muted', paused: 'paused', failed: 'muted' };
  const SIGN = {
    mute: '<i class="codicon codicon-mute" aria-hidden="true"></i>',
    pause: '<i class="codicon codicon-debug-pause" aria-hidden="true"></i>',
  };

  /** Mounts the stage in `host`; `post` sends to the extension; `layers` are the mark's images. */
  function create(host, { post, layers, onVoice }) {
    host.classList.add('voice');
    host.dataset.state = 'off';
    host.innerHTML = `<header class="voice-strip" role="toolbar" aria-label="Voice Mode">
    <span class="voice-state" id="voice-state" role="status">Off</span>
    <button class="voice-target" id="voice-target" aria-label="Who you are talking to">Overseer</button>
    <span class="grow"></span>
    <button id="voice-yes" hidden title="Yes to what Overseer read back (⌥⌘⇧Y)">Yes</button>
    <button id="voice-no" hidden title="No to what Overseer read back (⌥⌘⇧N)">No</button>
    <button id="voice-cancel" hidden title="Cancel the open request (⌥⌘⇧.)">Cancel</button>
    <button id="voice-mute" aria-pressed="false" aria-label="Mute"><i class="codicon codicon-mic" aria-hidden="true"></i></button>
  </header>
  <div class="voice-mark"><canvas id="voice-canvas" role="img" aria-label="The Overseer mark: it moves when Overseer hears you"></canvas><div class="voice-sign" id="voice-sign" hidden></div></div>
  <p class="voice-heard" id="voice-heard" aria-live="polite"></p>
  <p class="voice-said" id="voice-said" aria-live="polite"></p>
  <p class="voice-error" id="voice-error" role="alert" hidden></p>
  <div class="voice-meter" id="voice-meter" hidden aria-hidden="true"><i></i></div>
  <div class="voice-off" id="voice-off" hidden><span>Voice Mode is off.</span><span id="voice-off-reason"></span><button id="voice-on">Turn on</button></div>`;
    const $ = id => host.querySelector('#' + id);
    let voice = null, mark = null, heardFinal = '';
    const reducedQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
    let reducedSetting = false;
    window.__voice = { mark: null, messages: 0 };
    const shown = () => !host.hidden && host.offsetParent !== null;

    const load = src => new Promise((ok, fail) => { const i = new Image(); i.onload = () => ok(i); i.onerror = fail; i.src = src; });
    Promise.all(['core', 'swooshes', 'star', 'flat'].map(k => load(layers[k]).then(img => [k, img]))).then(pairs => {
      mark = window.OverseerVoiceMark.mount($('voice-canvas'), Object.fromEntries(pairs), { reduced: reducedQuery.matches || reducedSetting, active: shown });
      window.__voice.mark = mark;
      applyState();
    }).catch(() => {});
    reducedQuery.addEventListener('change', () => mark && mark.setReduced(reducedQuery.matches || reducedSetting));

    // A read-back or a plan that waits for a yes: Yes and No in the strip (and on the keyboard).
    let askReadBack = false, askPlan = null;
    const updateAsking = () => { const on = askReadBack || !!askPlan; $('voice-yes').hidden = !on; $('voice-no').hidden = !on; };
    // The listener's or the recognizer's last error, for ten minutes (AC-175).
    function showError(message, at) {
      const fresh = message && (!at || Date.now() - at < 600000);
      $('voice-error').hidden = !fresh;
      $('voice-error').textContent = fresh ? `Voice Mode had a problem: ${message}` : '';
    }

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
      if (mark) { mark.setState(m); mark.wake(); }
      const sign = m === 'muted' && state !== 'off' && state !== 'failed' ? 'mute' : m === 'paused' ? 'pause' : null;
      $('voice-sign').hidden = !sign;
      $('voice-sign').innerHTML = sign ? SIGN[sign] : '';
      host.dataset.state = state;
      $('voice-target').textContent = voice && voice.target_title ? voice.target_title : 'Overseer';
      $('voice-target').title = 'Who you are talking to (Voice Mode: Talk To…)';
      if (onVoice) onVoice(voice);
    }

    /** The open request, for Cancel in the strip (a spoken request going out or being thought about). */
    function setOpen(open) { $('voice-cancel').hidden = !open; $('voice-cancel').dataset.id = open ? open.id : ''; }

    function receive(m) {
      if (!m || typeof m !== 'object') return;
      window.__voice.messages += 1;
      if (m.type === 'snapshot') {
        voice = m.voice;
        reducedSetting = !!m.reducedMotion;
        if (mark) mark.setReduced(reducedQuery.matches || reducedSetting);
        $('voice-meter').hidden = !(reducedQuery.matches || reducedSetting);
        const err = voice && voice.listener && voice.listener.last_error;
        showError(err && err.message, err && err.at);
        applyState();
      } else if (m.type === 'live') {
        const v = m.msg;
        if (v.kind === 'state' && voice) { voice.state = v.state; voice.reason = v.reason; voice.enabled = v.state !== 'off'; applyState(); }
        else if (v.kind === 'level') {
          if (mark) mark.setLevel(v.source, v.value);
          $('voice-meter').firstElementChild.style.width = `${Math.round(v.value * 100)}%`;
          const log = window.__voice.levels || (window.__voice.levels = []);
          log.push({ t: performance.now(), source: v.source, value: v.value });
          if (log.length > 2000) log.shift();
        } else if (v.kind === 'heard') {
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
          if (v.request.state === 'waiting') askPlan = v.request.id;
          else if (askPlan === v.request.id) askPlan = null;
          updateAsking();
        } else if (v.kind === 'say') {
          // Every spoken line is also text (AC-174).
          $('voice-said').textContent = `Overseer: ${v.text}`;
        } else if (v.kind === 'spoke' && v.event === 'card_only') {
          $('voice-said').textContent = `Overseer (in the card, not said): ${v.text}`;
        } else if (v.kind === 'listener' && v.event === 'error') {
          showError(v.message, Date.now());
        } else if (v.kind === 'read_back') {
          askReadBack = !v.lapsed && !!v.agent; updateAsking();
        } else if (v.kind === 'confirm' && v.lapsed) {
          askPlan = null; updateAsking();
        } else if (v.kind === 'toast' && v.cancel) {
          askReadBack = false; updateAsking();
        } else if (v.kind === 'target' && voice) { voice.target = v.target; voice.target_title = v.target_title || (v.target === 'overseer' ? 'Overseer' : voice.target_title); applyState(); }
      }
    }

    $('voice-mute').addEventListener('click', () => post({ type: 'voiceMute' }));
    $('voice-on').addEventListener('click', () => post({ type: 'voiceToggle' }));
    $('voice-target').addEventListener('click', () => post({ type: 'voiceTarget' }));
    $('voice-cancel').addEventListener('click', e => post({ type: 'voiceCancel', id: e.currentTarget.dataset.id }));
    $('voice-yes').addEventListener('click', () => { askReadBack = false; askPlan = null; updateAsking(); post({ type: 'voiceAnswer', yes: true }); });
    $('voice-no').addEventListener('click', () => { askReadBack = false; askPlan = null; updateAsking(); post({ type: 'voiceAnswer', yes: false }); });
    void heardFinal;
    return { receive, setOpen, get voice() { return voice; }, wake() { if (mark) mark.wake(); } };
  }
  window.OverseerVoiceStage = { create };
})();
