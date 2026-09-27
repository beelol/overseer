#!/usr/bin/env node
// LIVE check of model downloads (AC-89) on this machine: a real `overseerd` with its own
// OVERSEER_HOME pulls models from the Ollama registry through the real Ollama, one at a time,
// smallest first. The second model is cancelled midway and pulled again, to show that the
// download continues from what it had. Nothing is loaded into memory.
//
//   node test/local/downloads-live.js qwen2.5-coder:1.5b qwen2.5-coder:3b ...
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const out = process.env.OUT || path.join(root, 'docs/verification/evidence/ac-89');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-live-'));
const env = { ...process.env, OVERSEER_HOME: home };
const G = 2 ** 30;
const gib = b => Math.round((b / G) * 100) / 100;
const results = [];
const log = [];
const say = line => { console.log(line); log.push(line); fs.mkdirSync(out, { recursive: true }); fs.writeFileSync(path.join(out, 'live.txt'), log.join('\n') + '\n'); };
const check = (name, ok, detail) => { results.push({ name, ok: !!ok }); say(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`); };
function ctl(method, params) {
  const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 120000, maxBuffer: 64 * 1024 * 1024 });
  const msg = JSON.parse(r.stdout.split('\n')[0] || '{"error":{"message":"no answer"}}');
  return msg.error ? { error: msg.error.message } : msg.result;
}
const sleep = ms => new Promise(r => setTimeout(r, ms));
const state = tag => (ctl('local.downloads').downloads || []).find(d => d.tag === tag);
const installed = () => ctl('local.inventory').models.map(m => m.tag);
const stamp = () => new Date().toISOString().slice(11, 19);

async function follow(tag, until) {
  let last = -1;
  for (;;) {
    const d = state(tag);
    if (d && d.percent !== null && d.percent !== last && (d.percent % 10 === 0 || d.status !== 'downloading')) {
      last = d.percent;
      say(`    ${stamp()}  ${tag}: ${d.status} ${d.percent}%  (${gib(d.completed)} of ${gib(d.total)} GiB)`);
    }
    if (d && until(d)) return d;
    await sleep(500);
  }
}

(async () => {
  const tags = process.argv.slice(2);
  const daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
  try {
    for (let i = 0; i < 100 && ctl('hello').error; i++) await sleep(100);
    const before = installed();
    say(`overseerd ${ctl('hello').version}; Ollama ${ctl('local.inventory').ollama.version}; free disk ${gib(ctl('local.inventory').disk_free)} GiB; installed: ${before.length} models`);
    const off = ctl('local.pull', { tag: tags[0], confirm: true });
    check('with downloads off the pull is refused and says why', /model downloads are off/.test(off.error || ''), off.error);
    ctl('settings.set', { values: { allowModelDownloads: true } });
    const ask = ctl('local.pull', { tag: tags[0] });
    check('the first pull ever asks for a confirmation, with the size', ask.needs_confirmation === true && ask.bytes > 0, `${tags[0]}: ${gib(ask.bytes)} GiB`);
    check('nothing was downloaded before the confirmation', !state(tags[0]) && !installed().includes(tags[0]));

    for (const [i, tag] of tags.entries()) {
      say(`\n== ${tag}`);
      if (installed().includes(tag)) { say('    already installed'); continue; }
      const started = Date.now();
      const s = ctl('local.pull', { tag, confirm: i === 0 });
      if (s.error) { check(`${tag} starts to download`, false, s.error); continue; }
      if (i === 1) {
        // Cancelled midway, then continued.
        const mid = await follow(tag, d => d.status !== 'starting' && (d.percent >= 30 || d.status !== 'downloading'));
        ctl('local.pull_cancel', { tag });
        const c = await follow(tag, d => d.status !== 'downloading' && d.status !== 'starting');
        check('a download is cancelled midway and the model is not installed', c.status === 'cancelled' && !installed().includes(tag), `cancelled at ${mid.percent}% (${gib(mid.completed)} GiB)`);
        const again = ctl('local.pull', { tag });
        let first = null;
        const done = await follow(tag, d => { if (first === null && d.status === 'downloading' && d.completed > 0) first = d.completed; return d.status === 'done' || d.status === 'failed'; });
        check('the next pull continues from what was downloaded', done.status === 'done' && first !== null && first >= mid.completed * 0.8, `the second pull's first progress was ${gib(first || 0)} GiB; the cancelled one had reached ${gib(mid.completed)} GiB${again.error ? '; ' + again.error : ''}`);
      } else {
        const done = await follow(tag, d => d.status === 'done' || d.status === 'failed' || d.status === 'cancelled');
        check(`${tag} is downloaded, with progress shown`, done.status === 'done', `${gib(done.total)} GiB in ${Math.round((Date.now() - started) / 1000)} s${done.reason ? '; ' + done.reason : ''}`);
      }
      const m = ctl('local.inventory').models.find(x => x.tag === tag);
      check(`${tag} is installed and reports tool calling`, m && m.capabilities.includes('tools'), m && `${gib(m.size)} GiB on disk, ${m.parameter_size}, ${m.quantization}, context up to ${m.max_context}`);
    }
    const events = ctl('events.list', { limit: 5000 }).events.filter(e => e.kind === 'local_download');
    check('progress was recorded as events', events.filter(e => e.payload.download.status === 'downloading').length >= tags.length, `${events.length} local_download events`);
    check('nothing was loaded into memory', ctl('local.inventory').loaded.length === 0);
    fs.writeFileSync(path.join(out, 'live-events.jsonl'), events.map(e => JSON.stringify({ ts: e.ts, download: e.payload.download, step: e.payload.step })).join('\n') + '\n');
  } catch (e) {
    check('the live check ran to its end', false, e.message);
  } finally {
    const failed = results.filter(r => !r.ok).length;
    say(`\n${results.length - failed} passed, ${failed} failed`);
    ctl('daemon.shutdown');
    daemon.kill();
    fs.rmSync(home, { recursive: true, force: true });
    process.exitCode = failed ? 1 : 0;
  }
})();
