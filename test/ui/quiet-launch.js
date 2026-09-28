// Launches a scenario's VS Code window on macOS without taking focus from whoever is using the Mac.
//
// `open -g` alone is not enough: VS Code's own CLI already launches through `open -n -g`, and the
// app still comes to the front, because Electron's BrowserWindow.show() (and a window created with
// show: true) calls [NSApp activateIgnoringOtherApps:YES]. So the app is started paused in its main
// process (--inspect-brk=0); before any of VS Code's code runs, the harness:
//   - makes show() and focus() show the window without activating the app (showInactive), and
//     app.focus() do nothing;
//   - sets a conditional breakpoint on VS Code's `new electron.BrowserWindow(options)` that turns
//     options.show off, then shows the new window inactive once Electron has created it.
// The window is fully rendered but transparent and click-through, so it neither covers the owner's
// apps nor catches their clicks, and it is never the active app. Keyboard and focus
// inside the page come from CDP (cdp.js enables focus emulation on every target).
//
// OVERSEER_UI_FOREGROUND=1 keeps the old foreground launch (for watching a run); other platforms
// always use it.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

/** The .app bundle that contains the `code` CLI, or null when it is not a macOS bundle. */
function appBundle(code) {
  const i = code.indexOf('.app/');
  return i === -1 ? null : code.slice(0, i + 4);
}

function wanted(code) {
  return process.platform === 'darwin' && process.env.OVERSEER_UI_FOREGROUND !== '1' && !!appBundle(code) && fs.existsSync(appBundle(code));
}

// Runs in VS Code's main process while it is paused on its first line.
const PATCH = `(() => {
  const { app, BrowserWindow } = require('electron');
  for (const proto of [BrowserWindow.prototype, Object.getPrototypeOf(BrowserWindow.prototype)]) {
    proto.show = function () { this.showInactive(); };
    proto.focus = function () { if (!this.isVisible()) this.showInactive(); };
  }
  app.focus = () => {};
  // What made the app active, if anything did: read back by activations().
  const t0 = Date.now(), seen = globalThis.__overseerActivations = [];
  const log = (what, extra) => seen.push({ ms: Date.now() - t0, what, ...extra });
  app.on('did-become-active', () => log('app became active'));
  app.on('browser-window-focus', () => log('window focused'));
  app.on('web-contents-created', (_e, wc) => {
    const proto = Object.getPrototypeOf(wc);
    if (proto.__overseerWrapped) return;
    proto.__overseerWrapped = true;
    const focus = proto.focus;
    proto.focus = function () { log('webContents.focus()', { stack: new Error().stack.split(String.fromCharCode(10)).slice(2, 6).map(l => l.trim()) }); return focus.apply(this, arguments); };
  });
  globalThis.__overseerQuietShow = 0;
  app.on('browser-window-created', (_event, win) => {
    log('window created', { show: globalThis.__overseerQuietShow > 0 });
    // An inactive window still sits on top of the owner's apps, where a click meant for them would
    // activate VS Code. Invisible and click-through, it covers nothing; CDP input and screenshots go
    // to the page itself, so the scenarios do not notice.
    win.setOpacity(0);
    win.setIgnoreMouseEvents(true);
    win.once('show', () => log('window shown'));
    if (globalThis.__overseerQuietShow > 0) {
      globalThis.__overseerQuietShow--;
      setImmediate(() => { if (!win.isDestroyed() && !win.isVisible()) win.showInactive(); });
    }
  });
  return 'patched';
})()`;

/**
 * Starts VS Code in the background. Returns { ready, quit, detachAfterFirstWindow, close }: `ready` resolves once the main process
 * is patched and running (it never rejects: on any failure the app is let run as it would anyway),
 * `quit` quits the app as its Quit menu item does, `close` drops the inspector connection.
 */
function launchQuiet({ code, args, env, root, note }) {
  const app = appBundle(code);
  const stamp = Date.now();
  const out = path.join(root, `code-${stamp}.log`), err = path.join(root, `code-err-${stamp}.log`);
  fs.writeFileSync(out, ''); fs.writeFileSync(err, '');
  // The environment VS Code's own CLI hands the app (cli.js: `open -n -g -a <app> --env ...`).
  const appEnv = { ...env, VSCODE_CLI: '1', ELECTRON_NO_ATTACH_CONSOLE: '1', VSCODE_CWD: process.cwd() };
  if (appEnv.NODE_OPTIONS !== undefined) { appEnv.VSCODE_NODE_OPTIONS = appEnv.NODE_OPTIONS; delete appEnv.NODE_OPTIONS; }
  delete appEnv.ELECTRON_RUN_AS_NODE;
  const envArgs = Object.entries(appEnv).filter(([k, v]) => k !== '_' && v !== undefined).flatMap(([k, v]) => ['--env', `${k}=${v}`]);
  const r = cp.spawnSync('open', ['-n', '-g', '-a', app, '--stdout', out, '--stderr', err, ...envArgs, '--args', '--inspect-brk=0', ...args], { env: {}, encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`open -g failed: ${(r.stderr || '').trim()}`);

  let socket, call;
  const ready = (async () => {
    let url;
    for (let i = 0; i < 300 && !url; i++) {
      url = (fs.readFileSync(err, 'utf8').match(/Debugger listening on (ws:\/\/\S+)/) || [])[1];
      if (!url) await delay(50);
    }
    if (!url) { note('background launch: no main-process inspector; the window may come to the front'); return; }
    socket = new WebSocket(url);
    await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', () => reject(new Error('inspector socket error')), { once: true }); });
    let next = 0; const pending = new Map(), scripts = [];
    let onPaused; const paused = new Promise(resolve => { onPaused = resolve; });
    socket.addEventListener('message', event => {
      const m = JSON.parse(event.data);
      if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
      else if (m.method === 'Debugger.scriptParsed') scripts.push(m.params);
      else if (m.method === 'Debugger.paused') onPaused(m.params);
    });
    call = (method, params = {}) => new Promise((resolve, reject) => {
      const id = ++next; const timer = setTimeout(() => { pending.delete(id); reject(new Error('inspector timeout: ' + method)); }, 20000);
      pending.set(id, m => { clearTimeout(timer); m.error ? reject(new Error(`${method}: ${m.error.message}`)) : resolve(m.result); });
      socket.send(JSON.stringify({ id, method, params }));
    });
    try {
      await call('Runtime.enable'); await call('Debugger.enable'); await call('Runtime.runIfWaitingForDebugger');
      // On the first line of VS Code's main.js, before any of it has run.
      if (!(await Promise.race([paused, delay(30000).then(() => false)]))) throw new Error('main process never paused');
      const patched = await call('Runtime.evaluate', { expression: PATCH, includeCommandLineAPI: true, returnByValue: true });
      if (patched.exceptionDetails) throw new Error(patched.exceptionDetails.exception?.description || patched.exceptionDetails.text);
      const main = scripts.find(s => /\/Resources\/app\/out\/main\.js$/.test(s.url));
      const source = main && (await call('Debugger.getScriptSource', { scriptId: main.scriptId })).scriptSource;
      const m = source && /new ([\w$]+)\.BrowserWindow\(([\w$]+)\)/.exec(source);
      if (!m) note('background launch: VS Code window constructor not found; the first window may come to the front');
      else {
        const before = source.slice(0, m.index), lineNumber = before.split('\n').length - 1, columnNumber = m.index - before.lastIndexOf('\n') - 1, o = m[2];
        await call('Debugger.setBreakpoint', { location: { scriptId: main.scriptId, lineNumber, columnNumber },
          condition: `(${o} && typeof ${o} === 'object' && ${o}.show !== false && (${o}.show = false, globalThis.__overseerQuietShow++), false)` });
      }
    } catch (error) {
      note('background launch: ' + error.message);
    } finally {
      await call('Debugger.resume').catch(() => {});
    }
  })().catch(error => note('background launch: ' + error.message));
  // The breakpoint lives as long as this connection, so it stays open until the window quits.
  return {
    ready,
    /** Everything that made the app active or focused a window since launch (empty when all went well). */
    activations: async () => {
      await ready;
      const r = call && await call('Runtime.evaluate', { expression: `JSON.stringify(globalThis.__overseerActivations || [])`, includeCommandLineAPI: true, returnByValue: true }).catch(() => null);
      try { return JSON.parse(r?.result?.value || '[]'); } catch { return []; }
    },
    // Cmd+Q is a menu shortcut of the active app, so it does nothing here: quit the way the menu does.
    // The inspector is then dropped: a process started with --inspect-brk waits for its debugger to
    // disconnect before it exits.
    quit: async () => {
      await ready;
      if (call) await call('Runtime.evaluate', { expression: `require('electron').app.quit()`, includeCommandLineAPI: true }).catch(() => {});
      try { socket?.close(); } catch {}
    },
    // For a launcher that exits (scripts/dev): waits until the first window exists, then lets go.
    // The patched show/focus stay; only windows opened after that may come to the front.
    detachAfterFirstWindow: async (ms = 90000) => {
      await ready;
      for (const end = Date.now() + ms; call && Date.now() < end; await delay(250)) {
        const r = await call('Runtime.evaluate', { expression: `require('electron').BrowserWindow.getAllWindows().some(w => w.isVisible())`, includeCommandLineAPI: true, returnByValue: true }).catch(() => null);
        if (r?.result?.value) break;
      }
      try { socket?.close(); } catch {}
    },
    close: () => { try { socket?.close(); } catch {} },
  };
}

module.exports = { launchQuiet, wanted };
