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
      else if (message.method === 'Input.dragIntercepted') this.dragData = message.params.data;
    });
  }

  /** Drags from one point to another with real HTML drag events (Input.setInterceptDrags). */
  async drag(from, to) {
    this.dragData = undefined;
    await this.call('Input.setInterceptDrags', { enabled: true }, this.workbench);
    await this.call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: from.x, y: from.y, button: 'none' }, this.workbench);
    await this.call('Input.dispatchMouseEvent', { type: 'mousePressed', x: from.x, y: from.y, button: 'left', clickCount: 1 }, this.workbench);
    for (let i = 1; i <= 6 && !this.dragData; i++) {
      await this.call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: from.x + (to.x - from.x) * i / 6, y: from.y + (to.y - from.y) * i / 6, button: 'left', buttons: 1 }, this.workbench);
      await delay(60);
    }
    const data = this.dragData;
    if (data) {
      for (const type of ['dragEnter', 'dragOver', 'drop']) { await this.call('Input.dispatchDragEvent', { type, x: to.x, y: to.y, data }, this.workbench); await delay(120); }
    }
    await this.call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: to.x, y: to.y, button: 'left', clickCount: 1 }, this.workbench);
    await this.call('Input.setInterceptDrags', { enabled: false }, this.workbench);
    return data;
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
    let socket;
    for (let attempt = 0; ; attempt++) {
      try {
        const lines = fs.readFileSync(file, 'utf8').trim().split('\n');
        socket = new WebSocket('ws://127.0.0.1:' + lines[0] + lines[1]);
        await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', () => reject(new Error('CDP socket error')), { once: true }); });
        break;
      } catch (error) {
        if (attempt > 40) throw error;
        await delay(500);
      }
    }
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
        // Behave as focused even when the test window is not the frontmost app (CDP input does not
        // activate the window; without this, focus() in webviews is dropped immediately).
        await this.call('Emulation.setFocusEmulationEnabled', { enabled: true }, sessionId).catch(() => {});
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
    const codes = { Enter: [13, 'Enter', '\r'], Escape: [27, 'Escape'], Tab: [9, 'Tab'], ArrowDown: [40, 'ArrowDown'], ArrowUp: [38, 'ArrowUp'], Backspace: [8, 'Backspace'], PageDown: [34, 'PageDown'], End: [35, 'End'], Home: [36, 'Home'], F10: [121, 'F10'], ContextMenu: [93, 'ContextMenu'], '.': [190, 'Period', '.'], ArrowRight: [39, 'ArrowRight'], ArrowLeft: [37, 'ArrowLeft'], Delete: [46, 'Delete'] };
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

  async click(x, y, { button = 'left' } = {}) {
    for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
      await this.call('Input.dispatchMouseEvent', { type, x, y, button, clickCount: 1 }, this.workbench);
    }
    await delay(80);
  }

  async move(x, y) {
    await this.call('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'none' }, this.workbench);
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

  /** Moves keyboard focus out of a webview so workbench shortcuts reach VS Code. */
  async focusWorkbench() {
    await this.evalWorkbench(`(() => { const a = document.activeElement; if (a && a.tagName === 'IFRAME') a.blur(); return true; })()`).catch(() => {});
  }

  /** Runs a command through the real command palette. */
  async command(title) {
    for (let attempt = 0; ; attempt++) {
      await this.focusWorkbench();
      await this.key('p', { meta: true, shift: true });
      try { await this.waitFor('!!document.querySelector(".quick-input-widget:not([style*=\\"display: none\\"]) input")', 5000, 'command palette'); break; }
      catch (error) {
        if (attempt >= 1) throw error;
        // Keyboard focus can stay inside a webview's own frame; click a neutral workbench spot.
        const p = await this.evalWorkbench(`(() => { const b = document.querySelector('.part.statusbar').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
        await this.click(p.x, p.y); await delay(300);
      }
    }
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
          await this.call('Emulation.setFocusEmulationEnabled', { enabled: true }, sessionId).catch(() => {});
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
