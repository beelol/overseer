// The Overseer mark in Voice Mode (AC-177): the Star animation the owner picked on 2026-09-27.
// GEOMETRY, MOTION, createMotion, step, pose and draw are copied unchanged from the reference,
// docs/design/voice-mark/index.html (test/unit/voice-mark.js checks they agree); change both
// together, and only by a recorded decision. `mount` runs it on a canvas from the daemon's state
// and levels, and draws nothing while the view is hidden.
(function (root) {
  'use strict';

  // Where the parts sit on the layers' 1254 px canvas (docs/design/brand/layers/split.py prints them).
  const GEOMETRY = { size: 1254, core: [632.8, 642.8], star: [637.4, 640.6], coreRadius: 283, fit: 0.74 };

  // Every number of the animation. Times in seconds; glow strength 0..1; scales are multiples of the star's size.
  const MOTION = {
    level: { rise: 0.030, fall: 0.160 },              // how fast the drawn level follows the voice
    glow: { rest: 0.18, breath: 0.05, breathPeriod: 4, hearing: 0.80, speaking: 0.35, reach: 0.40, reachHearing: 0.10 },
    star: { hearing: 0.85, speaking: 0.30, thinking: 0.12, light: 0.70, turn: 1.3, settle: 0.40 },
    rings: { threshold: 0.42, gap: 0.20, life: 0.95, reach: 1.05, width: 0.07, power: [0.35, 0.65], alpha: 0.90, max: 6 },
    still: { opacity: 0.45 },
    blend: 0.12,                                       // how fast listening and thinking fade in and out
  };

  const STATES = ['listening', 'hearing', 'thinking', 'speaking', 'muted', 'paused'];

  function createMotion() {
    return { clock: 0, hearing: 0, speaking: 0, listening: 1, thinking: 0, turn: 0, rings: [], lastRing: -Infinity, loud: false };
  }

  // Advances the motion by dt seconds. `level` is the voice's loudness (0..1): the owner's while
  // hearing, Overseer's while speaking; it is ignored in every other state.
  function step(m, state, level, dt) {
    const toward = (v, target, tau) => v + (target - v) * (1 - Math.exp(-dt / tau));
    const follow = (v, target) => toward(v, target, target > v ? MOTION.level.rise : MOTION.level.fall);
    m.clock += dt;
    m.hearing = follow(m.hearing, state === 'hearing' ? level : 0);
    m.speaking = follow(m.speaking, state === 'speaking' ? level : 0);
    m.listening = toward(m.listening, state === 'listening' ? 1 : 0, MOTION.blend);
    m.thinking = toward(m.thinking, state === 'thinking' ? 1 : 0, MOTION.blend);
    if (state === 'thinking') m.turn += MOTION.star.turn * dt;
    else {
      const quarter = Math.PI / 2;                    // the star has four points: a quarter turn looks like none
      m.turn = toward(m.turn, Math.round(m.turn / quarter) * quarter, MOTION.star.settle / 3);
    }
    const loud = state === 'speaking' && m.speaking > MOTION.rings.threshold;
    if (loud && !m.loud && m.clock - m.lastRing >= MOTION.rings.gap) {
      m.rings.push({ age: 0, power: MOTION.rings.power[0] + MOTION.rings.power[1] * m.speaking });
      m.lastRing = m.clock;
      if (m.rings.length > MOTION.rings.max) m.rings.shift();
    }
    m.loud = loud;
    for (const r of m.rings) r.age += dt / MOTION.rings.life;
    m.rings = m.rings.filter(r => r.age < 1);
    if (state === 'muted' || state === 'paused') m.rings = [];
  }

  // What to draw now. Pure: the same motion and state give the same pose.
  function pose(m, state, reduced) {
    const still = reduced || state === 'muted' || state === 'paused';
    const breath = MOTION.glow.breath * m.listening * Math.sin(2 * Math.PI * m.clock / MOTION.glow.breathPeriod);
    return {
      still,
      sign: state === 'muted' ? 'mute' : state === 'paused' ? 'pause' : null,
      meter: reduced && !(state === 'muted' || state === 'paused') ? Math.max(m.hearing, m.speaking) : null,
      glow: { strength: MOTION.glow.rest + breath + MOTION.glow.hearing * m.hearing + MOTION.glow.speaking * m.speaking,
              reach: MOTION.glow.reach + MOTION.glow.reachHearing * m.hearing },
      star: { scale: 1 + MOTION.star.hearing * m.hearing + MOTION.star.speaking * m.speaking + MOTION.star.thinking * m.thinking,
              turn: m.turn, light: MOTION.star.light * Math.max(m.hearing, m.speaking) },
      rings: m.rings.map(r => ({ radius: r.age * MOTION.rings.reach, alpha: (1 - r.age) * r.power })),
    };
  }

  // Draws a pose. `layers` holds the images core, swooshes, star and flat (the whole logo); `glow`
  // is the accent colour as "r, g, b". Back to front: glow, core, rings on the core, swooshes, star.
  function draw(ctx, spare, layers, p, glow) {
    const W = ctx.canvas.width, M = W * GEOMETRY.fit, S = GEOMETRY.size;
    const [cx, cy] = GEOMETRY.core.map(v => v / S), [sx, sy] = GEOMETRY.star.map(v => v / S);
    const place = (c, img, scale = 1, turn = 0, around = 'core') => {
      const fx = around === 'star' ? sx : cx, fy = around === 'star' ? sy : cy;
      c.save();
      c.translate(W / 2 + (fx - cx) * M, W / 2 + (fy - cy) * M);
      c.rotate(turn); c.scale(scale, scale);
      c.drawImage(img, -fx * M, -fy * M, M, M);
      c.restore();
    };
    ctx.clearRect(0, 0, W, W);
    if (p.still) { place(ctx, layers.flat); return; }

    const g = ctx.createRadialGradient(W / 2, W / 2, W * 0.12, W / 2, W / 2, W * p.glow.reach);
    g.addColorStop(0, `rgba(${glow}, ${0.55 * Math.max(0, p.glow.strength)})`);
    g.addColorStop(1, `rgba(${glow}, 0)`);
    ctx.fillStyle = g; ctx.fillRect(0, 0, W, W);

    place(ctx, layers.core);
    if (p.rings.length) {
      const s = spare.getContext('2d'), rim = GEOMETRY.coreRadius / S * M, band = W * MOTION.rings.width;
      s.globalCompositeOperation = 'source-over'; s.clearRect(0, 0, W, W);
      for (const r of p.rings) {
        const at = r.radius * rim, lg = s.createRadialGradient(W / 2, W / 2, Math.max(0, at - band), W / 2, W / 2, at + band);
        lg.addColorStop(0, 'rgba(255, 255, 255, 0)'); lg.addColorStop(0.5, `rgba(255, 255, 255, ${r.alpha})`); lg.addColorStop(1, 'rgba(255, 255, 255, 0)');
        s.fillStyle = lg; s.fillRect(0, 0, W, W);
      }
      s.globalCompositeOperation = 'destination-in'; place(s, layers.core); s.globalCompositeOperation = 'source-over';
      ctx.save(); ctx.globalCompositeOperation = 'screen'; ctx.globalAlpha = MOTION.rings.alpha; ctx.drawImage(spare, 0, 0); ctx.restore();
    }
    place(ctx, layers.swooshes);
    place(ctx, layers.star, p.star.scale, p.star.turn, 'star');
    if (p.star.light > 0.005) {
      ctx.save(); ctx.globalCompositeOperation = 'lighter'; ctx.globalAlpha = Math.min(1, p.star.light);
      place(ctx, layers.star, p.star.scale, p.star.turn, 'star'); ctx.restore();
    }
  }

  /** Runs the mark on `canvas`. `layers` are loaded images (core, swooshes, star, flat). */
  function mount(canvas, layers, options = {}) {
    const ctx = canvas.getContext('2d');
    const spare = document.createElement('canvas');
    const motion = createMotion();
    const levels = { owner: { value: 0, at: 0 }, overseer: { value: 0, at: 0 } };
    // workMs: the time a frame takes to compute and draw (it must stay under 16 ms for 60 frames a
    // second); frameMs: the interval between frames, which the display sets.
    // log: the star's size each frame (for tests that must see every syllable, not samples).
    const stats = { frames: 0, frameMs: [], workMs: [], hiddenFrames: 0, pose: null, state: 'listening', log: [] };
    let state = 'listening', reduced = !!options.reduced, last = 0, raf = 0, glow = '164, 139, 255';
    const probe = document.createElement('canvas').getContext('2d');
    const readGlow = () => {
      // The accent token, as "r, g, b".
      const token = getComputedStyle(document.documentElement).getPropertyValue('--ov-accent').trim() || '#a48bff';
      probe.fillStyle = '#000'; probe.fillStyle = token;
      const hex = probe.fillStyle;
      const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
      glow = m ? `${parseInt(m[1], 16)}, ${parseInt(m[2], 16)}, ${parseInt(m[3], 16)}` : (hex.match(/\d+/g) || [164, 139, 255]).slice(0, 3).join(', ');
    };
    readGlow();
    const size = () => {
      const px = Math.round((canvas.clientWidth || 300) * Math.min(2, window.devicePixelRatio || 1));
      if (canvas.width !== px) { canvas.width = canvas.height = spare.width = spare.height = px; }
    };
    const level = now => {
      const src = state === 'speaking' ? levels.overseer : levels.owner;
      return now - src.at > 150 ? 0 : src.value; // a level older than 150 ms is silence
    };
    const frame = now => {
      raf = 0;
      if (document.visibilityState === 'hidden') { stats.hiddenFrames += 1; return; }
      const dt = Math.min(0.05, (now - last) / 1000 || 0);
      if (last) { stats.frameMs.push(now - last); if (stats.frameMs.length > 600) stats.frameMs.shift(); }
      last = now;
      stats.frames += 1;
      const began = performance.now();
      size();
      step(motion, state, level(began), dt);
      const p = pose(motion, state, reduced);
      stats.pose = p; stats.state = state;
      draw(ctx, spare, layers, p, glow);
      stats.workMs.push(performance.now() - began); if (stats.workMs.length > 600) stats.workMs.shift();
      stats.log.push({ t: began, state, scale: p.star.scale, rings: p.rings.length }); if (stats.log.length > 1200) stats.log.shift();
      if (options.onPose) options.onPose(p);
      raf = requestAnimationFrame(frame);
    };
    const start = () => { if (!raf && document.visibilityState !== 'hidden') { last = 0; raf = requestAnimationFrame(frame); } };
    document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') { cancelAnimationFrame(raf); raf = 0; } else start(); });
    new MutationObserver(readGlow).observe(document.body, { attributes: true, attributeFilter: ['class'] });
    start();
    return {
      stats,
      setState(s) { state = STATES.includes(s) ? s : 'listening'; },
      setLevel(source, value) { const l = levels[source]; if (l) { l.value = Math.max(0, Math.min(1, Number(value) || 0)); l.at = performance.now(); } },
      setReduced(r) { reduced = !!r; },
    };
  }

  const api = { GEOMETRY, MOTION, STATES, createMotion, step, pose, draw, mount };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (root) root.OverseerVoiceMark = api;
})(typeof window !== 'undefined' ? window : null);
