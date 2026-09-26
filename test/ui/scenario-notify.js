// Packaged-UI scenario for AC-52 (no banners are shown: the daemon is pointed at a FAKE helper and
// a fake fallback). Checks that the real Overseer Notifier.app ships inside the installed VSIX
// (signed, bundle id, runs), that Overseer: Test Notification reports Overseer-native delivery,
// that a denied helper falls back and says so, and that the notification's click link
// (vscode://beelol.overseer/open-center) opens the Overseer view. The real banner, the permission
// prompt and the Notifications settings entry are owner-confirmed separately.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('notify');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    // Fake helper: logs its arguments; its exit code comes from a file the test controls.
    const fake = path.join(s.root, 'Fake Notifier.app');
    fs.mkdirSync(path.join(fake, 'Contents/MacOS'), { recursive: true });
    const log = path.join(s.root, 'notifier.log'), codeFile = path.join(s.root, 'notifier.code'), fallbackLog = path.join(s.root, 'fallback.log');
    fs.writeFileSync(codeFile, '0');
    fs.writeFileSync(path.join(fake, 'Contents/MacOS/notifier'), `#!/bin/sh\nprintf '%s\\n' "$@" >> '${log}'\nexit $(cat '${codeFile}')\n`, { mode: 0o755 });
    const fallback = path.join(s.root, 'fallback.sh');
    fs.writeFileSync(fallback, `#!/bin/sh\nprintf '%s|%s\\n' "$1" "$2" >> '${fallbackLog}'\n`, { mode: 0o755 });
    const repo = makeRepo(path.join(s.root, 'notify-demo'), { dirty: false });
    s.settings({ 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    // The real helper as installed from the VSIX.
    const ext = path.join(s.extensions, fs.readdirSync(s.extensions).find(d => d.startsWith('beelol.overseer')));
    const app = path.join(ext, 'bin', 'Overseer Notifier.app');
    const verify = cp.spawnSync('codesign', ['--verify', '--deep', app], { encoding: 'utf8' });
    const id = cp.spawnSync('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleIdentifier', path.join(app, 'Contents/Info.plist')], { encoding: 'utf8' }).stdout.trim();
    const name = cp.spawnSync('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleName', path.join(app, 'Contents/Info.plist')], { encoding: 'utf8' }).stdout.trim();
    const archs = cp.spawnSync('lipo', ['-archs', path.join(app, 'Contents/MacOS/notifier')], { encoding: 'utf8' }).stdout.trim();
    const status = cp.spawnSync(path.join(app, 'Contents/MacOS/notifier'), ['--status'], { encoding: 'utf8', timeout: 15000 });
    check('the installed extension ships a signed Overseer Notifier.app (bundle id, name, universal, icon) that runs', verify.status === 0 && id === 'com.beelol.overseer.notifier' && name === 'Overseer' && /arm64/.test(archs) && /x86_64/.test(archs) && fs.existsSync(path.join(app, 'Contents/Resources/AppIcon.icns')) && /^(notDetermined|denied|authorized|provisional|ephemeral)$/.test(status.stdout.trim()),
      { verify: verify.status, id, name, archs, status: status.stdout.trim(), statusExit: status.status });

    s.launch(repo, { OVERSEER_NOTIFIER_APP: fake, OVERSEER_TEST_NOTIFIER_DIRECT: '1', OVERSEER_NOTIFY_FALLBACK: fallback });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const toast = pattern => cdp.waitFor(`[...document.querySelectorAll('.notification-toast')].map(t => t.innerText).find(t => ${pattern}.test(t)) || null`, 20000).catch(() => null);

    // Allowed: Overseer-native delivery, with the click link.
    await cdp.command('Overseer: Test Notification');
    const okToast = await toast('/test notification/');
    const args = fs.existsSync(log) ? fs.readFileSync(log, 'utf8') : '';
    check('Test Notification is delivered by the Overseer helper with the Overseer-view link', /Sent a test notification from Overseer/.test(okToast || '') && args.includes('--open\nvscode://beelol.overseer/open-center') && !fs.existsSync(fallbackLog), { okToast, args });
    await s.screenshot('test-notification-native');
    await cdp.command('Notifications: Clear All Notifications');

    // Denied: falls back and says so.
    fs.writeFileSync(codeFile, '3');
    await cdp.command('Overseer: Test Notification');
    const deniedToast = await toast('/not as Overseer/');
    check('with notifications denied it falls back, delivers anyway and tells the user how to fix it', /overseer-notifier \(denied\); fell back to/.test(deniedToast || '') && /Allow Overseer in System Settings/.test(deniedToast || '') && fs.existsSync(fallbackLog), { deniedToast });
    await s.screenshot('test-notification-denied');
    await cdp.command('Notifications: Clear All Notifications');

    // A notification click opens vscode://beelol.overseer/open-center: route it inside this window.
    // VS Code asks once before an extension handles a vscode:// link; choose "Do not ask me again".
    await cdp.command('Developer: Open URL');
    await cdp.input('', 'vscode://beelol.overseer/open-center');
    const prompt = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const box = d.querySelector('.dialog-checkbox-row .monaco-checkbox, .monaco-custom-toggle'); const open = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === 'Open'); if (!open) return null; const r = open.getBoundingClientRect(), c = box?.getBoundingClientRect(); return { text: d.innerText, x: r.left + r.width / 2, y: r.top + r.height / 2, cx: c && c.left + c.width / 2, cy: c && c.top + c.height / 2 }; })()`, 20000, 'URI prompt');
    await s.screenshot('vscode-uri-prompt');
    if (prompt.cx) { await cdp.click(prompt.cx, prompt.cy); await delay(300); }
    await cdp.click(prompt.x, prompt.y);
    const view = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list')`, 30000).catch(() => null);
    check('the notification link opens the Overseer view (after VS Code\'s one-time "Allow … to open this URI?")', !!view && /Allow 'Overseer' extension to open this URI/.test(prompt.text), prompt.text);
    // Second time: no prompt; the view is revealed again.
    await cdp.command('View: Close All Editors'); await delay(800);
    await cdp.command('Developer: Open URL');
    await cdp.input('', 'vscode://beelol.overseer/open-center');
    const again = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list')`, 20000).then(() => true, () => false);
    const asked = await cdp.evalWorkbench(`!!document.querySelector('.monaco-dialog-box')`);
    check('later links open the Overseer view without asking again', again && !asked, { again, asked });
    await s.screenshot('link-opened-center');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    // Unregister this test's temporary copy of the notifier from LaunchServices (it shares the
    // real app's bundle id; stale copies confuse macOS about which app "Overseer" is).
    try { const ext = fs.readdirSync(s.extensions).find(d => d.startsWith('beelol.overseer')); cp.spawnSync('/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister', ['-u', path.join(s.extensions, ext, 'bin', 'Overseer Notifier.app')]); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
