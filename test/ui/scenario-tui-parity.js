// Packaged-UI scenario for the terminal UI's two checks that need a VS Code window (docs/rfcs/tui.md),
// with fixture programs, the simulated voice and the Claude fixture as Overseer (no microphone, no
// paid turn). The terminal's part is played by the daemon calls the TUI itself makes (tui/src/app.rs):
//   T-29  with an agent's review open in VS Code, a change is accepted the way the TUI's `a` does
//         (comparison.options, workspace.hunks, review.accept with the hunk's lines), a whole file
//         accepted as `A` does (each change in turn) and then rejected as `R` does (review.reject for
//         each change): VS Code's review shows the change Accepted, the file Accepted, then the file
//         back as it was (gone from the review, its text the comparison's again).
//   T-35  with VS Code's Voice Mode open (home with Voice Mode on), the simulated voice is driven
//         through every state the way the TUI's keys do (voice.set on, mute, unmute, off) and the
//         simulated room (a voice, a spoken request, another app taking the microphone). VS Code's
//         shown states are recorded in the page; a terminal client subscribed as the TUI subscribes
//         (hello as "tui", voice.subscribe, the live "voice" states) records its own; the two match.
//         What VS Code shows is what the owner sees: home turned into the voice view with its strip's
//         state, and the status bar's words (Voice Mode turned on from the terminal shows as on).
const fs = require('fs');
const net = require('net');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, git, repoRoot } = require('./harness');

(async () => {
  const s = new Session('tui-parity');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const micUsers = path.join(s.root, 'mic-users');
  let terminal = null;
  try {
    const repo = makeRepo(path.join(s.root, 'shop'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    fs.writeFileSync(micUsers, '');
    s.launch(repo, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER_TEST_VOICE: '1', OVERSEER_LISTENER_TEST_MIC_USERS: micUsers,
    });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const click = async (frame, selector) => { const p = await s.webviewPoint(frame, selector); await cdp.click(p.x, p.y); await delay(400); };

    // ---------------- T-29: Accept and Reject from the terminal, seen in VS Code's review.
    const created = s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', title: 'Terminal review', prompt: '',
      args: ['-c', "sed -i '' -e 's/^L10: original$/L10: agent edit/' -e 's/^L100: original$/L100: agent edit/' a.txt; sed -i '' -e 's/^L50: original$/L50: agent edit/' -e 's/^L250: original$/L250: agent edit/' b.txt"] });
    const runId = created.run.id, ws = created.workspace.id, W = created.workspace.path;
    for (let i = 0; i < 80 && !['completed', 'failed', 'interrupted'].includes(s.ctl('state').runs.find(r => r.id === runId).status); i++) await delay(250);
    const line = (file, n) => fs.readFileSync(path.join(W, file), 'utf8').split('\n')[n - 1];
    const bBefore = git(W, 'show', 'HEAD:b.txt') + '\n';

    // VS Code's review of the agent, open while the terminal works.
    await s.selectAgent('Terminal review', { settle: 2000 });
    const chat = await cdp.webview(`window.__overseer?.selected?.() === ${JSON.stringify(runId)} && !!document.getElementById('review')`, 30000);
    await click(chat, '#review');
    const rv = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('diffs') && !document.getElementById('compare').hidden`, 60000);
    const fileOf = file => `[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)})`;
    const waitFile = (file, pred, ms = 20000) => rv.waitFor(`(() => { const e = ${fileOf(file)}; if (!e) return false; e.scrollIntoView({ block: 'start' }); return e.dataset.loadState === 'rendered' && (${pred}); })()`, ms).then(() => true, () => false);
    const fileState = file => rv.eval(`(() => { const e = ${fileOf(file)}; if (!e) return null;
      return { hunks: Number(e.dataset.hunks || 0), accepted: Number(e.dataset.reviewed || 0), fileButton: e.querySelector('.file-accept')?.textContent, filePressed: e.querySelector('.file-accept')?.getAttribute('aria-pressed'),
        changes: [...e.querySelectorAll('.hunk-accept')].map(b => ({ word: b.textContent.trim(), pressed: b.getAttribute('aria-pressed') })) }; })()`);
    const readyA = await waitFile('a.txt', 'Number(e.dataset.hunks) === 2 && Number(e.dataset.reviewed || 0) === 0');
    const readyB = await waitFile('b.txt', 'Number(e.dataset.hunks) === 2 && Number(e.dataset.reviewed || 0) === 0');
    check('fixture: VS Code\'s review shows a.txt and b.txt with two changes each, none accepted', readyA && readyB, { a: await fileState('a.txt'), b: await fileState('b.txt') });
    await s.screenshot('t29-review-before');

    // What the TUI loads on `v`: the daemon's default comparison, then a file's changes.
    const options = s.ctl('comparison.options', { run_id: runId }).options;
    const chosen = options.find(o => o.default && o.available && typeof o.base === 'string') || options.find(o => o.available && typeof o.base === 'string');
    const hunks = file => s.ctl('workspace.hunks', { workspace_id: ws, path: file, base: chosen.base, run_id: runId }).hunks;
    // The TUI's send_accept and send_reject (tui/src/app.rs), each change after the last one answered.
    const tuiAccept = (file, h) => s.ctl('review.accept', { run_id: runId, path: file, key: h.key, modified_start: h.modified_start, modified_lines: h.modified_lines, base_lines: h.base_lines });
    const tuiReject = (file, h) => s.ctl('review.reject', { workspace_id: ws, path: file, base: chosen.base, key: h.key });

    // `a` on a.txt's first change (L10).
    const aHunks = hunks('a.txt');
    const l10 = aHunks.find(h => JSON.stringify(h.modified_lines) === '["L10: agent edit"]');
    const sentAt = Date.now();
    tuiAccept('a.txt', l10);
    const shownA = await waitFile('a.txt', "Number(e.dataset.reviewed) === 1 && [...e.querySelectorAll('.hunk-accept')].some(b => b.getAttribute('aria-pressed') === 'true' && b.textContent.trim() === 'Accepted')");
    const tookMs = Date.now() - sentAt;
    const afterA = await fileState('a.txt');
    const marks = s.ctl('review.marks', { run_id: runId });
    await s.screenshot('t29-change-accepted-in-terminal');
    check('T-29: a change accepted from the terminal is in the daemon\'s marks and reads Accepted in VS Code\'s review (the other change not), the agent\'s line kept',
      chosen.mode === 'task_start' && shownA && marks.keys.includes(l10.key) && afterA.accepted === 1 && afterA.changes.filter(c => c.word === 'Accepted').length === 1 && line('a.txt', 10) === 'L10: agent edit', { comparison: chosen.mode, tookMs, afterA, marks: marks.keys });

    // `A` on b.txt: every change, in turn.
    for (const h of hunks('b.txt')) tuiAccept('b.txt', h);
    const shownB = await waitFile('b.txt', "Number(e.dataset.reviewed) === 2 && e.querySelector('.file-accept').textContent === 'Accepted' && e.querySelector('.file-accept').getAttribute('aria-pressed') === 'true'");
    const afterAll = await fileState('b.txt');
    await s.screenshot('t29-file-accepted-in-terminal');
    check('T-29: a whole file accepted from the terminal reads Accepted in VS Code (each change and the file\'s button)', shownB && afterAll.changes.length === 2 && afterAll.changes.every(c => c.word === 'Accepted'), afterAll);

    // `R` on b.txt: every change rejected, in turn.
    for (const h of hunks('b.txt')) tuiReject('b.txt', h);
    const bGone = await rv.waitFor(`!${fileOf('b.txt')} && document.body.dataset.checking === 'false'`, 20000).then(() => true, () => false);
    const bText = fs.readFileSync(path.join(W, 'b.txt'), 'utf8');
    const afterR = await rv.eval(`({ files: [...document.querySelectorAll('.diff-file .file-path')].map(e => e.textContent), count: document.getElementById('total').dataset.count })`);
    const aStill = await waitFile('a.txt', "Number(e.dataset.reviewed) === 1");
    await s.screenshot('t29-file-rejected-in-terminal');
    check('T-29: a whole file rejected from the terminal is back as it was on disk and leaves VS Code\'s review; a.txt keeps its accepted change', bGone && bText === bBefore && JSON.stringify(afterR.files) === '["a.txt"]' && afterR.count === '1' && aStill, { ...afterR, same: bText === bBefore, aStill });

    // ---------------- T-35: Voice Mode in VS Code and in a terminal at the same time.
    const ext = fs.readdirSync(s.extensions).find(d => d.startsWith('beelol.overseer'));
    const socketPath = cp.execFileSync(path.join(s.extensions, ext, 'bin', `overseerd-${process.platform}-${process.arch}`), ['socket-path'], { env: s.baseEnv(), encoding: 'utf8' }).trim();
    // A terminal client: subscribes and keeps the state as the TUI does (App::on_voice, voice_state).
    terminal = await new Promise((resolve, reject) => {
      const sock = net.createConnection(socketPath);
      const seen = []; let voice = null, buf = '';
      const note = () => { const st = voice ? (voice.enabled === true ? voice.state || 'starting' : 'off') : null; if (st && seen[seen.length - 1] !== st) seen.push(st); };
      sock.setEncoding('utf8');
      sock.on('connect', () => { sock.write(JSON.stringify({ id: 1, method: 'hello', params: { client: 'tui' } }) + '\n'); sock.write(JSON.stringify({ id: 2, method: 'voice.subscribe', params: {} }) + '\n'); });
      sock.on('error', reject);
      sock.on('data', chunk => {
        buf += chunk;
        for (let i; (i = buf.indexOf('\n')) >= 0;) {
          const text = buf.slice(0, i); buf = buf.slice(i + 1);
          let m; try { m = JSON.parse(text); } catch { continue; }
          if (m.id === 2) { if (m.error) { reject(new Error(m.error.message)); return; } voice = m.result.voice; note(); resolve({ seen, now: () => seen[seen.length - 1], close: () => sock.destroy() }); }
          else if (m.method === 'voice' && m.params && m.params.kind === 'state' && voice) { voice.state = m.params.state; voice.reason = m.params.reason; voice.enabled = m.params.state !== 'off'; note(); }
        }
      });
    });

    // Overseer's conversation in VS Code, where Voice Mode shows: what it shows is recorded in the
    // page, from what the owner sees (home turned into the voice view, and its strip's state).
    await cdp.command('Overseer: Talk to Overseer'); await delay(1500);
    const home = await s.editorView(`!!document.getElementById('home-voice-toggle') && !!document.getElementById('home-voice')`);
    const SHOWN = `(document.body.dataset.voice === 'on' && !document.getElementById('home-voice').hidden ? document.getElementById('home-voice').dataset.state : 'off')`;
    await home.eval(`(() => { const now = () => ${SHOWN}; const log = window.__tuiParity = [now()];
      const note = () => { const x = now(); if (log[log.length - 1] !== x) log.push(x); };
      const mo = new MutationObserver(note); mo.observe(document.body, { attributes: true, attributeFilter: ['data-voice'] }); mo.observe(document.getElementById('home-voice'), { attributes: true, attributeFilter: ['data-state', 'hidden'] }); })()`);
    const vsSeen = () => home.eval(`window.__tuiParity`);
    const LABEL = { listening: 'Listening', hearing: 'Hearing you', thinking: 'Thinking', speaking: 'Speaking', muted: 'Muted', paused: 'Paused for a call' };
    const statusBar = () => cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].map(e => e.textContent.trim()).filter(t => /^(Starting|Listening|Hearing you|Thinking|Speaking|Muted|Paused for a call|Voice stopped)$/.test(t))`);
    const shown = (state, ms) => home.waitFor(`${SHOWN} === ${JSON.stringify(state)}`, ms).then(() => true, () => false);
    const both = async (state, ms, label) => {
      const vs = await shown(state, ms);
      for (let i = 0; i < 20 && terminal.now() !== state; i++) await delay(100);
      // The status bar redraws on its own (through the main process): give it up to 3 s.
      const want = b => state === 'off' ? b.length === 0 : b.includes(LABEL[state]);
      let bar = await statusBar();
      for (let i = 0; i < 30 && !want(bar); i++) { await delay(100); bar = await statusBar(); }
      const strip = await home.eval(`document.getElementById('home-voice').hidden ? '' : document.querySelector('#home-voice .home-voice-state').textContent.trim()`);
      if (label) await s.screenshot(label);
      return { vs, terminal: terminal.now() === state, statusBar: bar, strip, words: want(bar) && strip === (state === 'off' ? '' : LABEL[state]) };
    };
    const at = {};
    at.off = await both('off', 5000, 't35-off');
    s.ctl('voice.set', { enabled: true }); // ctrl+v
    at.listening = await both('listening', 30000, 't35-listening');
    s.ctl('voice.simulate', { speechlike: 3, words: '' });
    at.hearing = await both('hearing', 15000, 't35-hearing');
    await shown('listening', 15000);
    s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sleep', args: ['120'], prompt: '', title: 'Phone' });
    s.ctl('voice.say', { text: 'Tell Phone to use the new wire format.' });
    at.thinking = await both('thinking', 15000, 't35-thinking');
    at.speaking = await both('speaking', 60000, 't35-speaking');
    await shown('listening', 60000);
    s.ctl('voice.set', { muted: true }); // ctrl+t
    at.muted = await both('muted', 10000, 't35-muted');
    s.ctl('voice.set', { muted: false }); // ctrl+t
    await shown('listening', 30000);
    fs.writeFileSync(micUsers, 'us.zoom.xos\n'); // another app takes the microphone (the simulated room's record)
    at.paused = await both('paused', 10000, 't35-paused');
    fs.writeFileSync(micUsers, '');
    await shown('listening', 20000);
    s.ctl('voice.set', { enabled: false }); // ctrl+v
    at.offAgain = await both('off', 10000, 't35-off-again');
    await delay(1000);
    const vsSeq = await vsSeen(), tuiSeq = [...terminal.seen];
    const seven = ['off', 'listening', 'hearing', 'thinking', 'speaking', 'muted', 'paused'];
    check('T-35: each state shows in VS Code (home\'s voice strip and the status bar, in words) and in the terminal at the same time', Object.values(at).every(x => x.vs && x.terminal && x.words), at);
    check('T-35: VS Code showed every state (off, listening, hearing you, thinking, speaking, muted, paused for a call)', seven.every(x => vsSeq.includes(x)), vsSeq);
    check('T-35: the terminal was told the same states as VS Code showed, in the same order', JSON.stringify(vsSeq) === JSON.stringify(tuiSeq), { vscode: vsSeq, terminal: tuiSeq });
    result.voice = { vscode: vsSeq, terminal: tuiSeq };
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { terminal?.close(); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
