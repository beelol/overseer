// AC-177: the mark in Overseer (extension/media/voice-mark.js) moves exactly as the reference the
// owner picked (docs/design/voice-mark/index.html): the same levels give the same poses in every
// state, within 1%; and each state does what the RFC's table says.
// Run: node test/unit/voice-mark.js
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const root = path.resolve(__dirname, '../..');
const ours = require(path.join(root, 'extension/media/voice-mark.js'));
const html = fs.readFileSync(path.join(root, 'docs/design/voice-mark/index.html'), 'utf8');
const start = html.indexOf("// Where the parts sit on the layers' 1254 px canvas");
const end = html.indexOf('// ---------------------------------------------------------------- this page only');
const sandbox = {};
vm.runInNewContext(html.slice(start, end) + '\nthis.ref = { MOTION, GEOMETRY, createMotion, step, pose };', sandbox);
const ref = sandbox.ref;

let failures = 0;
const check = (name, ok, detail = '') => { if (ok) console.log('ok  ', name); else { failures++; console.log('FAIL', name, detail); } };

check('MOTION is the reference\'s', JSON.stringify(ours.MOTION) === JSON.stringify(ref.MOTION), JSON.stringify(ours.MOTION));
check('GEOMETRY is the reference\'s', JSON.stringify(ours.GEOMETRY) === JSON.stringify(ref.GEOMETRY));

// A voice: syllables four a second, with gaps, the same for both.
const voice = t => { const f = (t * 4.6) % 1; return Math.floor(t * 4.6) % 7 === 3 ? 0 : 0.8 * Math.pow(Math.sin(Math.PI * f), 1.4); };
const script = [['listening', 3], ['hearing', 5], ['thinking', 1.5], ['speaking', 4], ['listening', 2], ['muted', 1], ['paused', 1], ['listening', 1]];
const near = (a, b) => Math.abs(a - b) <= 0.01 * Math.max(1, Math.abs(a), Math.abs(b));
const same = (p, q) => p.still === q.still && p.sign === q.sign && near(p.glow.strength, q.glow.strength) && near(p.glow.reach, q.glow.reach) && near(p.star.scale, q.star.scale) && near(p.star.turn, q.star.turn) && near(p.star.light, q.star.light) && p.rings.length === q.rings.length && p.rings.every((r, i) => near(r.radius, q.rings[i].radius) && near(r.alpha, q.rings[i].alpha));

for (const reduced of [false, true]) {
  const a = ours.createMotion(), b = ref.createMotion();
  let t = 0, bad = null, frames = 0;
  const seen = {};
  const first = {};
  for (const [state, secs] of script) {
    const segment = [];
    for (let i = 0; i < secs * 60; i++) {
      const dt = 1 / 60; t += dt;
      const level = state === 'hearing' || state === 'speaking' ? voice(t) : 0;
      ours.step(a, state, level, dt); ref.step(b, state, level, dt);
      const p = ours.pose(a, state, reduced), q = ref.pose(b, state, reduced);
      frames++;
      if (!bad && !same(p, q)) bad = { state, t, ours: p, ref: q };
      (seen[state] = seen[state] || []).push(p);
      segment.push(p);
    }
    if (!first[state]) first[state] = segment;
  }
  check(`the same poses as the reference in every state, ${frames} frames${reduced ? ', reduced motion' : ''}`, !bad, JSON.stringify(bad));
  if (reduced) {
    check('reduced motion: a still mark with a level meter', seen.hearing.every(p => p.still && p.meter !== null) && seen.muted.every(p => p.meter === null));
    continue;
  }
  const max = (list, f) => Math.max(...list.map(f));
  const min = (list, f) => Math.min(...list.map(f));
  const settled = list => list.slice(Math.floor(list.length / 2));
  // Listening: the star at rest, a faint glow that breathes (±5% around 18%).
  const listening = settled(first.listening);
  check('listening: the star at rest', max(listening, p => Math.abs(p.star.scale - 1)) < 0.02);
  check('listening: a faint glow that breathes', max(listening, p => p.glow.strength) <= 0.24 && min(listening, p => p.glow.strength) >= 0.12 && max(listening, p => p.glow.strength) - min(listening, p => p.glow.strength) > 0.05);
  // Hearing: the star grows with the voice, up to 1.85x, and the glow swells.
  check('hearing: the star grows with the voice, never past 1.85x', max(seen.hearing, p => p.star.scale) > 1.5 && max(seen.hearing, p => p.star.scale) <= 1.851);
  check('hearing: the star falls back between syllables', min(settled(seen.hearing), p => p.star.scale) < 1.25);
  check('hearing: the glow swells', max(seen.hearing, p => p.glow.strength) > 0.6);
  // Thinking: turns and sits 12% larger, not tied to sound.
  const thinking = settled(seen.thinking);
  check('thinking: the star turns', max(thinking, p => p.star.turn) - min(thinking, p => p.star.turn) > 0.3);
  check('thinking: about 12% larger', thinking.every(p => p.star.scale > 1.08 && p.star.scale < 1.13));
  // Speaking: up to 1.3x, rings of light cross the core.
  check('speaking: the star grows with Overseer\'s voice, never past 1.3x', max(seen.speaking, p => p.star.scale) <= 1.301 && max(seen.speaking, p => p.star.scale) > 1.15);
  check('speaking: rings of light cross the core', max(seen.speaking, p => p.rings.length) >= 1 && seen.speaking.some(p => p.rings.some(r => r.radius > 0.8)));
  check('speaking: no rings while hearing', seen.hearing.every(p => p.rings.length === 0));
  // Muted and paused: still, with their sign.
  check('muted: still, with the mute sign', seen.muted.every(p => p.still && p.sign === 'mute'));
  check('paused: still, with the pause sign', seen.paused.every(p => p.still && p.sign === 'pause'));
  // After thinking the star comes to rest on a quarter turn.
  const last = seen.listening[seen.listening.length - 1];
  const quarter = Math.PI / 2;
  check('after thinking the star rests on a quarter turn', Math.abs(last.star.turn / quarter - Math.round(last.star.turn / quarter)) < 0.02, last.star.turn);
}

// Timing: from silence, the star follows a voice within 100 ms.
{
  const m = ours.createMotion();
  for (let i = 0; i < 60; i++) ours.step(m, 'hearing', 0, 1 / 60);
  let t = 0;
  while (ours.pose(m, 'hearing', false).star.scale < 1 + 0.85 * 0.9 * 0.9) { ours.step(m, 'hearing', 0.9, 1 / 60); t += 1 / 60; if (t > 1) break; }
  check('the star follows the voice within 100 ms', t <= 0.1, `${(t * 1000).toFixed(0)} ms`);
}

console.log(failures ? `${failures} check(s) failed` : 'the mark matches the reference in every state');
process.exit(failures ? 1 : 0);
