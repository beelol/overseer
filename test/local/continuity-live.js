#!/usr/bin/env node
// LIVE check of Continuity's foundations on this machine (AC-83, AC-85, AC-86, AC-140): the real
// network, the real memory and the real Ollama, through a real `overseerd` with its own
// OVERSEER_HOME. It reads facts, asks the guard about every installed model, and loads ONE model:
// the daemon's own pick, which is inside the memory budget, under the load watchdog; it is
// unloaded again at the end. Nothing is downloaded and no account or paid token is used.
//
//   node test/local/continuity-live.js [--no-load]     (OUT=<folder> for the evidence)
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync, execFileSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const out = process.env.OUT || path.join(root, 'docs/verification/evidence/ac-85');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-live-'));
const env = { ...process.env, OVERSEER_HOME: home };
const G = 2 ** 30;
const gib = b => Math.round((b / G) * 10) / 10;
const results = [];
const log = [];
const say = line => { console.log(line); log.push(line); };
function check(name, ok, detail) {
  results.push({ name, ok: !!ok, detail });
  say(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}
function ctl(method, params) {
  const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 900000 });
  const msg = JSON.parse(r.stdout.split('\n')[0] || '{"error":{"message":"no answer"}}');
  return msg.error ? { error: msg.error.message } : msg.result;
}
const ollama = async (p, body) => (await fetch('http://127.0.0.1:11434' + p, body ? { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) } : undefined)).json();
const sh = (cmd, args) => execFileSync(cmd, args, { encoding: 'utf8' });
const redact = s => s.split(os.homedir()).join('~').split(os.userInfo().username).join('USER');

function vmStat() {
  const vm = sh('/usr/bin/vm_stat', []);
  const page = Number(/page size of (\d+)/.exec(vm)[1]);
  const pages = name => Number(new RegExp(`${name}:\\s+(\\d+)`).exec(vm)[1]);
  return { page, available: (pages('Pages free') + pages('Pages inactive') + pages('Pages purgeable')) * page };
}

(async () => {
  const daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
  try {
    for (let i = 0; i < 100 && ctl('hello').error; i++) await new Promise(r => setTimeout(r, 100));
    const hello = ctl('hello');
    say(`overseerd ${hello.version} (protocol ${hello.protocol}); ${sh('/usr/bin/sw_vers', ['-productVersion']).trim()} ${os.arch()}; Ollama ${(await ollama('/api/version')).version}`);

    // ---- AC-83: the system's own answer and the probes, on the real network
    const conn = ctl('connection.status').status;
    say(`connection: ${conn.state} (${conn.reason}); ${conn.system.detail}`);
    say(`  baseline by name: ${conn.baseline.by_name.reason}; by IP: ${conn.baseline.by_ip.reason}`);
    for (const [id, h] of Object.entries(conn.providers)) say(`  ${id}: ${h.reachable ? 'reachable' : 'unreachable'} (${h.reason}, ${h.source})`);
    check('the system itself says this machine is connected, with the flags it decided from', conn.system.state === 'connected' && /flags 0x/.test(conn.system.detail));
    check('both baseline probes and both providers answer', conn.baseline.by_name.ok && conn.baseline.by_ip.ok && Object.values(conn.providers).every(h => h.reachable === true));
    check('the state is online', conn.state === 'online');

    // ---- AC-85: memory and models against the system's own tools, at the same moment
    const before = vmStat();
    const inv = ctl('local.inventory');
    const after = vmStat();
    const total = Number(sh('/usr/sbin/sysctl', ['-n', 'hw.memsize']).trim());
    const level = Number(sh('/usr/sbin/sysctl', ['-n', 'kern.memorystatus_level']).trim());
    const pressure = /System-wide memory free percentage: (\d+)%/.exec(sh('/usr/bin/memory_pressure', []));
    const expected = (before.available + after.available) / 2;
    const apart = Math.abs(inv.memory.available - expected) / expected;
    say(`memory: total ${gib(inv.memory.total)} GiB, available ${gib(inv.memory.available)} GiB, pressure ${inv.memory.pressure}, level ${inv.memory.level}% (${inv.memory.source})`);
    check('total memory is exactly sysctl hw.memsize', inv.memory.total === total, `${total} bytes`);
    check('available memory is within 5% of vm_stat', apart < 0.05, `daemon ${gib(inv.memory.available)} GiB, vm_stat ${gib(expected)} GiB, ${(apart * 100).toFixed(1)}% apart`);
    check('the free percentage is within 5 points of kern.memorystatus_level and memory_pressure', Math.abs(inv.memory.level - level) <= 5 && pressure && Math.abs(inv.memory.level - Number(pressure[1])) <= 5, `daemon ${inv.memory.level}%, sysctl ${level}%, memory_pressure ${pressure && pressure[1]}%`);
    const tags = (await ollama('/api/tags')).models;
    check('every installed model is reported, and no other', inv.models.length === tags.length && tags.every(t => inv.models.some(m => m.tag === t.name && m.size === t.size)), `${tags.length} models`);
    let matched = 0;
    for (const t of tags) {
      const show = await ollama('/api/show', { model: t.name });
      const m = inv.models.find(x => x.tag === t.name);
      const arch = show.model_info['general.architecture'];
      const kv = show.model_info[`${arch}.attention.head_count_kv`];
      const heads = Array.isArray(kv) ? kv : Array(show.model_info[`${arch}.block_count`]).fill(kv);
      const ctx = /num_ctx\s+(\d+)/.exec(show.parameters || '');
      const same = m.parameters === show.model_info['general.parameter_count'] && m.max_context === show.model_info[`${arch}.context_length`] && JSON.stringify(m.geometry.kv_heads_per_layer) === JSON.stringify(heads)
        && JSON.stringify(m.capabilities) === JSON.stringify(show.capabilities) && m.family === t.details.family && m.quantization === t.details.quantization_level && (m.configured_context || null) === (ctx ? Number(ctx[1]) : null)
        && m.base === (t.details.parent_model || show.details.parent_model || t.name);
      if (same) matched++; else say(`  differs: ${t.name}`);
      say(`  ${t.name.padEnd(26)} ${String(gib(m.size)).padStart(5)} GiB on disk  ${String(m.parameter_size).padEnd(7)} ${m.quantization}  context ${m.configured_context || 'default'} of ${m.max_context}  ${m.capabilities.join(',')}`);
    }
    check('size, family, quantization, context, capabilities and geometry match /api/tags and /api/show', matched === tags.length, `${matched} of ${tags.length}`);
    const ps = (await ollama('/api/ps')).models;
    check('loaded models match /api/ps', inv.loaded.length === ps.length && ps.every(p => inv.loaded.some(l => l.tag === p.name && l.size === p.size)), `${ps.length} loaded`);
    check('Ollama is reported as installed and running, with its version', inv.ollama.running && inv.ollama.installed && inv.ollama.version === (await ollama('/api/version')).version, `${inv.ollama.version} at ${redact(inv.ollama.installed)}`);
    check('free disk where the models live is reported', inv.disk_free > 0, `${gib(inv.disk_free)} GiB`);

    // ---- AC-86: the budget and the pick on this machine
    const picked = ctl('local.pick');
    const b = picked.pick.budget, c = picked.pick.chosen;
    say(`budget: ${gib(b.budget)} GiB = min(${b.ceiling_percent}% of ${gib(b.total)} GiB = ${gib(b.ceiling_share)} GiB, ${gib(b.available)} GiB available − ${gib(b.headroom)} GiB headroom = ${gib(b.ceiling_now)} GiB)`);
    say(`pick: ${c ? `${c.tag} at a ${c.context / 1024}k context, ${gib(c.bytes)} GiB (${c.measured ? 'measured' : 'estimated'}), run as ${c.run_tag}` : 'none'}`);
    for (const r of picked.pick.rejected) say(`  not taken: ${r.tag}: ${r.reason}`);
    check('the pick is qwen3-coder:30b at 64k or more', c && c.tag === 'qwen3-coder:30b' && c.context >= 65536);
    check('the pick is inside the budget', c && c.bytes <= c.budget.budget, c && `${gib(c.bytes)} GiB of ${gib(c.budget.budget)} GiB`);
    check('a ceiling over 50% is refused', /60 was refused/.test(ctl('settings.set', { values: { ramCeilingPercent: 60 } }).error || ''));

    // ---- AC-140: the guard, asked about every installed model at the smallest context
    let refused = 0;
    for (const m of inv.models) {
      const a = ctl('local.approve', { tag: m.tag, context: 16384 });
      say(`  guard, ${m.tag} at 16k: ${a.error ? 'REFUSED: ' + a.error.split(' (')[0] : `allowed, ${gib(a.bytes)} GiB of ${gib(a.budget.budget)} GiB`}`);
      if (a.error) refused++;
      if (!a.error && a.bytes > a.budget.budget) check(`the guard never allows a model over the budget (${m.tag})`, false);
    }
    const huge = ctl('local.approve', { tag: 'qwen3.5:122b', context: 16384 });
    check('qwen3.5:122b is refused, with the arithmetic', /^qwen3\.5:122b is too big to load: [\d.]+ GiB at a 16k context is over the budget of [\d.]+ GiB/.test(huge.error || ''), huge.error);
    const hugeLoad = ctl('local.load', { tag: 'qwen3.5:122b', context: 16384 });
    check('a load of qwen3.5:122b is refused the same way, and nothing is loaded', /too big to load/.test(hugeLoad.error || '') && (await ollama('/api/ps')).models.length === ps.length);

    // ---- one guarded load of the pick, measured, then unloaded
    if (!process.argv.includes('--no-load') && c) {
      const est = ctl('local.approve', { tag: c.tag, context: c.context });
      const started = Date.now();
      const l = ctl('local.load', { tag: c.tag, context: c.context });
      if (l.error) check('the pick loads under the watchdog', false, l.error);
      else {
        const m = l.detail.measured;
        say(`loaded ${l.detail.run_tag} in ${Math.round((Date.now() - started) / 100) / 10} s; ${l.loaded.samples} memory samples, lowest available ${gib(l.loaded.lowest_available)} GiB; available before ${gib(l.detail.memory_before.available)} GiB, after ${gib(l.detail.memory_after.available)} GiB`);
        check('the pick loads under the watchdog, with memory sampled while it loads', l.loaded.samples >= 1, `${l.loaded.samples} samples`);
        check('Ollama\'s own measurement is recorded', m && m.size > 0 && m.context === c.context, m && `${gib(m.size)} GiB at a ${m.context / 1024}k context`);
        const apart2 = Math.abs(est.bytes - m.size) / m.size;
        check('the estimate is within 15% of the measured size', est.measured || apart2 < 0.15, est.measured ? 'a measurement from an earlier load was already in use' : `estimate ${gib(est.bytes)} GiB, measured ${gib(m.size)} GiB, ${(apart2 * 100).toFixed(1)}% apart`);
        const again = ctl('local.pick').pick.chosen;
        check('the next pick uses the measurement', again.measured && again.bytes === m.size, `${gib(again.bytes)} GiB`);
        const mem = ctl('local.inventory').memory;
        check('memory pressure stayed normal with the model loaded', mem.pressure === 'normal', `level ${mem.level}%, available ${gib(mem.available)} GiB`);
        ctl('local.unload', { tag: l.detail.run_tag });
        for (let i = 0; i < 40 && (await ollama('/api/ps')).models.some(x => x.name === l.detail.run_tag); i++) await new Promise(r => setTimeout(r, 250));
        check('the model is unloaded again', !(await ollama('/api/ps')).models.some(x => x.name === l.detail.run_tag), `available ${gib(ctl('local.inventory').memory.available)} GiB`);
      }
    }
    const failed = results.filter(r => !r.ok).length;
    say(`\n${results.length - failed} passed, ${failed} failed`);
    fs.mkdirSync(out, { recursive: true });
    fs.writeFileSync(path.join(out, 'live.txt'), redact(log.join('\n')) + '\n');
    fs.writeFileSync(path.join(out, 'live.json'), redact(JSON.stringify({ at: new Date().toISOString(), results, connection: conn, memory: inv.memory, ollama: inv.ollama, models: inv.models.map(m => ({ ...m, geometry: undefined })), pick: picked.pick }, null, 1)) + '\n');
    process.exitCode = failed ? 1 : 0;
  } finally {
    ctl('daemon.shutdown');
    daemon.kill();
    fs.rmSync(home, { recursive: true, force: true });
  }
})();
