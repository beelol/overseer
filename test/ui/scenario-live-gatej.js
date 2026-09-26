// LIVE scenario for Gate J (AC-55, AC-60, AC-62) on the OWNER'S daemon: tiny prompts, one attempt
// per step, no retries. Claude Code (existing login, haiku) and Codex (ChatGPT A, gpt-5.6-luna),
// plus one tiny Codex turn on ChatGPT B for usage. Through the daemon API the composer uses: an
// attached image and a mentioned worktree file reach the agent (its reply names the color and the
// file's first line); per-turn model / effort / permission mode reach the harness (argv read from
// the run's launch record, never its environment); a running turn is interrupted and the next
// message answered; a finished run continues after the daemon restarts. Then usage per
// account is compared with the harness's own output, and the chats are captured in both Overseer
// themes at 900 and 1600 px. Refuses to start if any run is active on the owner's daemon.
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

const RED_PNG = 'iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nGO4IydHU8QwasGoBaMWjFowasGoBaMWjFowasGoBaMWDBULAJI2YD1ZaHIvAAAAAElFTkSuQmCC';
const DATA = path.join(os.homedir(), 'Library/Application Support/Overseer');
const ACTIVE = ['queued', 'starting', 'running', 'waiting_for_user'];
const CHATGPT_A = process.env.CHATGPT_A || 'p-f262c1bc4958';
const CHATGPT_B = process.env.CHATGPT_B || 'p-52fb6421edd2';

(async () => {
  const s = new Session('live-gatej', { ownerDaemon: true });
  const result = { checks: [], runs: {}, usage: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  let cdp;
  try {
    // Safety: the owner's daemon must be idle before anything is installed or restarted.
    const ownerBin = path.join(os.homedir(), '.vscode/extensions', fs.readdirSync(path.join(os.homedir(), '.vscode/extensions')).find(d => d.startsWith('beelol.overseer')), 'bin', `overseerd-${process.platform}-${process.arch}`);
    const ownerCtl = (m, p = {}) => JSON.parse(cp.execFileSync(ownerBin, ['ctl', m, JSON.stringify(p)], { env: s.baseEnv(), encoding: 'utf8' }).split('\n')[0]);
    let before;
    try { before = ownerCtl('state').result; } catch { before = null; }
    if (before) {
      const busy = before.runs.filter(r => ACTIVE.includes(r.status));
      const clients = ownerCtl('daemon.clients').result.vscode;
      s.note('owner daemon before', { runs: before.runs.length, active: busy.length, vscodeClients: clients, pid: before.daemon.pid });
      if (busy.length || clients) throw new Error(`owner daemon busy (active runs ${busy.length}, VS Code windows ${clients}); not touching it`);
      ownerCtl('daemon.shutdown'); await delay(1500);
      s.note('stopped the idle owner daemon so the new build starts');
    }

    const repo = makeRepo(path.join(s.root, 'live-gatej'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, {});
    cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const waitDone = async (id, ms = 180000) => { for (let t = 0; t < ms; t += 1000) { const r = run(id); if (!ACTIVE.includes(r.status)) return r; await delay(1000); } return run(id); };
    const turns = id => s.ctl('run.turns', { run_id: id });
    const replies = id => s.ctl('events.list', { run_id: id, limit: 5000 }).events.filter(e => e.kind === 'output' && e.payload?.role === 'assistant').map(e => e.payload.text || '');
    const argvOf = (id, proc) => { try { const l = JSON.parse(fs.readFileSync(path.join(DATA, 'runs', id, proc, 'launch.json'), 'utf8')); return l.args || l.argv || null; } catch { return null; } };
    const procs = id => { try { return fs.readdirSync(path.join(DATA, 'runs', id)).filter(p => /^p\d+$/.test(p)).sort((a, b) => Number(a.slice(1)) - Number(b.slice(1))); } catch { return []; } };
    const flag = (argv, f) => { const i = (argv || []).indexOf(f); return i >= 0 ? argv[i + 1] : undefined; };

    // Gate K: agents are selected in the side bar; the chat is the editor view.
    let dash;
    const select = async id => {
      await s.selectRun(id, { settle: 2000 });
      dash = await s.editorView(`document.body.dataset.runId === ${JSON.stringify(id)} || document.body.dataset.mode === 'chat'`, 30000);
      await dash.waitFor(`document.body.dataset.mode === 'chat' && !document.getElementById('send').disabled`, 60000);
    };
    const shots = async label => {
      for (const theme of ['Overseer Dark', 'Overseer Light']) {
        const cur = JSON.parse(fs.readFileSync(path.join(s.profile, 'User/settings.json'), 'utf8')); cur['workbench.colorTheme'] = theme;
        fs.writeFileSync(path.join(s.profile, 'User/settings.json'), JSON.stringify(cur, null, 2)); await delay(1500);
        for (const w of [1600, 900]) { await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: 1000, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1500); await s.screenshot(`${label}-${theme.split(' ')[1].toLowerCase()}-${w}`); }
      }
      await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
      const cur = JSON.parse(fs.readFileSync(path.join(s.profile, 'User/settings.json'), 'utf8')); cur['workbench.colorTheme'] = 'Overseer Dark';
      fs.writeFileSync(path.join(s.profile, 'User/settings.json'), JSON.stringify(cur, null, 2)); await delay(1200);
    };

    const SPECS = [
      { key: 'claude', harness: 'claude', profile: 'system-claude', model: 'haiku', effort: 'low', mode: 'Plan only', modeValue: 'plan', modeFlag: ['--permission-mode', 'plan'], effortFlag: '--effort', stopPrompt: 'Write a numbered list of 60 short facts about lighthouses.' },
      { key: 'codex', harness: 'codex', profile: CHATGPT_A, model: 'gpt-5.6-luna', effort: 'low', mode: 'Read only', modeValue: 'read-only', modeFlag: ['-s', 'read-only'], effortFlag: '-c', stopPrompt: 'Write a numbered list of 60 short facts about lighthouses.' },
    ].filter(x => !process.env.ONLY || process.env.ONLY.split(',').includes(x.key));

    // Turns go through the daemon API the composer uses (run.follow_up with the same options and
    // image payload), so this live check does not depend on composer keyboard focus.
    const follow = (id, prompt, extra = {}) => s.ctl('run.follow_up', { run_id: id, prompt, ...extra });
    for (const spec of SPECS) {
      const r = {};
      result.runs[spec.key] = r;
      const t = s.ctl('task.create', { repo, harness: spec.harness, profile_id: spec.profile, model: spec.model, prompt: 'Reply with exactly: ready', title: `Live ${spec.key}` });
      r.id = t.run.id;
      const first = await waitDone(r.id);
      r.first = { status: first.status, reply: replies(r.id).pop() };

      // Image + a mentioned worktree file + options for this turn.
      follow(r.id, 'What color is the attached image, and what is the first line of README.md? Answer in one short line.\n\nFiles mentioned (paths relative to the repository root): `README.md`',
        { model: spec.model, effort: spec.effort, permission_mode: spec.modeValue, images: [{ mime: 'image/png', data: RED_PNG }] });
      await delay(1500);
      const second = await waitDone(r.id);
      const p = procs(r.id); const argv = argvOf(r.id, p[p.length - 1]);
      r.rich = { status: second.status, reply: replies(r.id).pop(), argv: argv && argv.filter(a => a.length < 200).slice(0, 40) };
      const effortOk = spec.harness === 'claude' ? flag(argv, '--effort') === 'low' : (argv || []).some(a => /model_reasoning_effort="?low"?/.test(a));
      const modeOk = (argv || []).some((a, i) => (a === spec.modeFlag[0] && argv[i + 1] === spec.modeFlag[1]) || (spec.harness === 'codex' && a === 'sandbox_mode="read-only"'));
      const imageOk = spec.harness === 'claude' ? true : (argv || []).includes('-i');
      check(`${spec.key}: an attached image and a mentioned worktree file reach the agent (reply names red and "# fixture")`, /red/i.test(r.rich.reply || '') && /fixture/i.test(r.rich.reply || '') && imageOk, { reply: r.rich.reply });
      check(`${spec.key}: per-turn model, effort and permission mode reach the harness`, flag(argv, spec.harness === 'claude' ? '--model' : '-m') === spec.model && effortOk && modeOk, { argv: r.rich.argv });

      // Stop and send: interrupt a longer turn, then send the new message (what ⌥Enter does).
      follow(r.id, 'Write a numbered list of 60 short facts about lighthouses.');
      for (let i = 0; i < 30 && run(r.id).status !== 'running'; i++) await delay(500);
      await delay(3000);
      s.ctl('run.interrupt', { run_id: r.id });
      await waitDone(r.id, 60000);
      follow(r.id, 'Stop. Reply with exactly: stopped ok');
      await delay(1500);
      await waitDone(r.id);
      const st = turns(r.id);
      r.stop = st.map(x => [x.n, x.prompt.slice(0, 40), x.status]);
      check(`${spec.key}: a running turn is interrupted and the next message is answered`, st.length >= 4 && st[2].status === 'interrupted' && /stopped ok/i.test(replies(r.id).pop() || ''), r.stop);
    }

    // The chats, as the dashboard shows them.
    for (const spec of SPECS) { await select(result.runs[spec.key].id); await delay(1500); await s.screenshot(`${spec.key}-chat`); }

    // Continue finished runs after the daemon restarts.
    s.ctl('daemon.shutdown'); await delay(2000);
    await cdp.command('Overseer: Start Daemon');
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'reconnected');
    await delay(2000);
    for (const spec of SPECS) {
      const r = result.runs[spec.key];
      const nativeBefore = run(r.id).native_id;
      follow(r.id, 'Reply with exactly: resumed ok');
      await delay(1500);
      const done = await waitDone(r.id);
      r.resumed = { status: done.status, reply: replies(r.id).pop(), sameSession: run(r.id).native_id === nativeBefore };
      check(`${spec.key}: a finished run continues its session after the daemon restarts`, done.status === 'completed' && /resumed ok/i.test(r.resumed.reply || '') && r.resumed.sameSession, r.resumed);
    }

    // One tiny turn on ChatGPT B, so both ChatGPT accounts report usage.
    if (!process.env.ONLY || process.env.ONLY.includes('codex')) {
      const b = s.ctl('task.create', { repo, harness: 'codex', profile_id: CHATGPT_B, model: 'gpt-5.6-luna', prompt: 'Reply with exactly: ok', title: 'Live ChatGPT B' });
      result.runs.chatgptB = { id: b.run.id, status: (await waitDone(b.run.id)).status };
    }

    // Usage: what the daemon reports per account vs the harness's own output.
    await cdp.command('Overseer: Refresh Account Status'); await delay(3000);
    for (const id of ['system-claude', CHATGPT_A, CHATGPT_B, 'system-opencode']) result.usage[id] = s.ctl('account.usage', { id });
    s.note('account.usage', result.usage);
    // Claude: the last rate_limit_event in the run's raw output.
    const raw = { claude: null, codex: null };
    if (result.runs.claude) {
      for (const p of procs(result.runs.claude.id).reverse()) {
        const dir = path.join(DATA, 'runs', result.runs.claude.id, p);
        for (const f of fs.readdirSync(dir).filter(f => f.startsWith('output-')).sort().reverse()) {
          // Shim records wrap each harness line: {"d": "<line>", "s": "o", "t": ms}.
          const rec = fs.readFileSync(path.join(dir, f), 'utf8').split('\n').reverse().find(l => l.includes('rate_limit_event'));
          if (rec && !raw.claude) { try { raw.claude = JSON.parse(JSON.parse(rec).d); } catch {} }
        }
        if (raw.claude) break;
      }
    }
    result.raw = raw;
    s.note('raw claude rate_limit_event', raw.claude);
    const cu = result.usage['system-claude'];
    const win = raw.claude?.rate_limit_info?.unifiedWindows || {};
    const same = (label, key) => { const w = (cu?.windows || []).find(x => x.label === label); return !win[key] || (w && Math.abs(w.used - win[key].utilization) < 1e-9 && w.resets_at_ms === win[key].resetsAt * 1000); };
    check('Claude usage matches its own rate_limit_event (utilization and reset time per window)', cu && cu.reported && !!raw.claude && same('5 hours', 'five_hour') && same('week', 'seven_day'), { usage: cu, raw: raw.claude?.rate_limit_info });
    for (const id of [CHATGPT_A, CHATGPT_B]) {
      const u = result.usage[id];
      check(`Codex ${id === CHATGPT_A ? 'ChatGPT A' : 'ChatGPT B'} usage comes from its session log (or says not reported)`, u && (u.reported ? (u.windows || []).length > 0 && /session/.test(u.source || '') : true), u);
    }
    check('OpenCode says not reported', result.usage['system-opencode']?.reported === false, result.usage['system-opencode']);
    await s.openOverseerView(); await delay(800);
    await s.screenshot('accounts-usage');

    // AC-55: live chats in both themes at 1600 and 900 px.
    for (const spec of SPECS) { await select(result.runs[spec.key].id); await delay(1200); await shots(`chat-${spec.key}`); }
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    // Leave nothing running: interrupt anything this scenario started that is still active.
    try { for (const r of Object.values(result.runs)) if (r.id && ACTIVE.includes(s.ctl('state').runs.find(x => x.id === r.id)?.status)) s.ctl('run.interrupt', { run_id: r.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); try { s.ctl('daemon.shutdown'); } catch {} }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
