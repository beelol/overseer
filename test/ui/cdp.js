// Test-only Chrome DevTools Protocol driver for a real VS Code window.
// Adapted from Branch Diff's test/webview-driver.js (MIT). Drives the workbench with
// real keyboard input, reads DOM state, captures screenshots and reaches webviews.
const fs = require('fs');
const path = require('path');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

class Cdp {
  constructor(socket) {
    this.socket = socket; this.next = 0; this.pending = new Map(); this.contexts = new Map(); this.errors = [];
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.id) {
        const pending = this.pending.get(message.id); if (!pending) return;
        this.pending.delete(message.id); clearTimeout(pending.timer);
        if (message.error) pending.reject(new Error(message.error.message)); else pending.resolve(message.result);
      } else if (message.method === 'Runtime.executionContextCreated') {
        this.contexts.set(message.sessionId + ':' + message.params.context.id, { sessionId: message.sessionId, id: message.params.context.id, origin: message.params.context.origin });
      } else if (message.method === 'Runtime.executionContextDestroyed') {
        this.contexts.delete(message.sessionId + ':' + message.params.executionContextId);
      } else if (message.method === 'Runtime.exceptionThrown') this.errors.push(message.params.exceptionDetails);
    });
  }

  call(method, params = {}, sessionId) {
    return new Promise((resolve, reject) => {
      const id = ++this.next;
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('CDP timeout: ' + method)); }, 20000);
      this.pending.set(id, { resolve, reject, timer }); this.socket.send(JSON.stringify({ id, method, params, sessionId }));
    });
  }

  static async connect(profileDir) {
    const file = path.join(profileDir, 'DevToolsActivePort');
    for (let i = 0; i < 120 && !fs.existsSync(file); i++) await delay(250);
    const lines = fs.readFileSync(file, 'utf8').trim().split('\n');
    const socket = new WebSocket('ws://127.0.0.1:' + lines[0] + lines[1]);
    await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }); });
    const cdp = new Cdp(socket);
    await cdp.attachWorkbench();
    return cdp;
  }

  async attachWorkbench() {
    for (let attempt = 0; attempt < 120; attempt++) {
      const { targetInfos } = await this.call('Target.getTargets');
      const page = targetInfos.find(t => t.type === 'page' && t.url.includes('workbench'));
      if (page) {
        const { sessionId } = await this.call('Target.attachToTarget', { targetId: page.targetId, flatten: true });
        this.workbench = sessionId;
        await this.call('Runtime.enable', {}, sessionId);
        await this.call('Page.enable', {}, sessionId);
        for (let i = 0; i < 80; i++) {
          if (await this.evalWorkbench('!!document.querySelector(".monaco-workbench .part.activitybar")').catch(() => false)) return;
          await delay(250);
        }
      }
      await delay(250);
    }
    throw new Error('VS Code workbench page not found');
  }

  async evalWorkbench(expression) {
    const result = await this.call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }, this.workbench);
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result?.value;
  }

  async screenshot(file) {
    const { data } = await this.call('Page.captureScreenshot', { format: 'png' }, this.workbench);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, Buffer.from(data, 'base64'));
    return file;
  }

  async key(key, { meta = false, shift = false, ctrl = false, alt = false } = {}) {
    const modifiers = (alt ? 1 : 0) | (ctrl ? 2 : 0) | (meta ? 4 : 0) | (shift ? 8 : 0);
    const codes = { Enter: [13, 'Enter', '\r'], Escape: [27, 'Escape'], Tab: [9, 'Tab'], ArrowDown: [40, 'ArrowDown'], ArrowUp: [38, 'ArrowUp'], Backspace: [8, 'Backspace'], PageDown: [34, 'PageDown'] };
    const [keyCode, code, text] = codes[key] || [key.toUpperCase().charCodeAt(0), 'Key' + key.toUpperCase()];
    const base = { modifiers, windowsVirtualKeyCode: keyCode, nativeVirtualKeyCode: keyCode, key: codes[key] ? key : (shift ? key.toUpperCase() : key), code };
    await this.call('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...base }, this.workbench);
    if (text && !meta && !ctrl) await this.call('Input.dispatchKeyEvent', { type: 'char', ...base, text }, this.workbench);
    await this.call('Input.dispatchKeyEvent', { type: 'keyUp', ...base }, this.workbench);
    await delay(60);
  }

  async type(text) {
    await this.call('Input.insertText', { text }, this.workbench);
    await delay(80);
  }

  async click(x, y) {
    for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
      await this.call('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1 }, this.workbench);
    }
    await delay(80);
  }

  async wheel(x, y, deltaY) {
    await this.call('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY }, this.workbench);
    await delay(80);
  }

  async waitFor(expression, timeout = 20000, label = expression) {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      try { last = await this.evalWorkbench(expression); if (last) return last; } catch (e) { last = e.message; }
      await delay(200);
    }
    throw new Error(`Timed out waiting for ${label} (last: ${JSON.stringify(last)})`);
  }

  /** Runs a command through the real command palette. */
  async command(title) {
    await this.key('p', { meta: true, shift: true });
    await this.waitFor('!!document.querySelector(".quick-input-widget:not([style*=\\"display: none\\"]) input")', 5000, 'command palette');
    await this.type(title);
    await delay(400);
    await this.key('Enter');
    await delay(300);
  }

  quickInputState() {
    return this.evalWorkbench(`(() => { const w = document.querySelector('.quick-input-widget'); if (!w || w.style.display === 'none') return null;
      return { title: w.querySelector('.quick-input-title')?.textContent || '', rows: [...w.querySelectorAll('.monaco-list-row')].map(r => r.getAttribute('aria-label') || r.textContent).slice(0, 20), value: w.querySelector('input')?.value }; })()`);
  }

  async waitQuickTitle(fragment, timeout = 20000) {
    return this.waitFor(`(() => { const w = document.querySelector('.quick-input-widget'); return !!w && w.style.display !== 'none' && (w.querySelector('.quick-input-title')?.textContent || '').includes(${JSON.stringify(fragment)}); })()`, timeout, 'quick input ' + fragment);
  }

  /** Picks a quick-pick item by typing a filter and pressing Enter. */
  async pick(titleFragment, filter) {
    await this.waitQuickTitle(titleFragment);
    if (filter) await this.type(filter);
    await delay(300);
    await this.key('Enter');
    await delay(300);
  }

  async input(titleFragment, text) {
    await this.waitQuickTitle(titleFragment);
    if (text) await this.type(text);
    await delay(150);
    await this.key('Enter');
    await delay(300);
  }

  /** Finds a webview execution context whose document satisfies `probe`. */
  async webview(probe, timeout = 30000) {
    const attached = new Set();
    const end = Date.now() + timeout;
    while (Date.now() < end) {
      const { targetInfos } = await this.call('Target.getTargets');
      for (const target of targetInfos.filter(t => ['iframe', 'webview', 'page'].includes(t.type))) {
        if (attached.has(target.targetId)) continue;
        try {
          const { sessionId } = await this.call('Target.attachToTarget', { targetId: target.targetId, flatten: true });
          await this.call('Runtime.enable', {}, sessionId); attached.add(target.targetId);
        } catch {}
      }
      for (const context of [...this.contexts.values()].reverse()) {
        try {
          const result = await this.call('Runtime.evaluate', { expression: probe, contextId: context.id, returnByValue: true }, context.sessionId);
          if (result.result?.value) return new Frame(this, context);
        } catch {}
      }
      await delay(300);
    }
    throw new Error('Webview not found for probe ' + probe);
  }

  close() { this.socket.close(); }
}

class Frame {
  constructor(cdp, context) { this.cdp = cdp; this.context = context; }
  async eval(expression) {
    const result = await this.cdp.call('Runtime.evaluate', { expression, contextId: this.context.id, returnByValue: true, awaitPromise: true }, this.context.sessionId);
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result?.value;
  }
  async waitFor(expression, timeout = 20000) {
    const end = Date.now() + timeout; let last;
    while (Date.now() < end) {
      try { last = await this.eval(expression); if (last) return last; } catch (e) { last = e.message; }
      await delay(200);
    }
    throw new Error(`Timed out in webview waiting for ${expression} (last: ${JSON.stringify(last)})`);
  }
  async key(key, opts) { return this.cdp.call('Input.dispatchKeyEvent', {}, this.context.sessionId).catch(() => {}); }
}

module.exports = { Cdp, delay };
