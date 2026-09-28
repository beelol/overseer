// Pair a Phone (AC-117): the pairing code as a QR code and as text, the time it still works, and
// what to do on the phone. The extension sends the state; this page draws it. The QR code's
// modules come from the extension (made on this Mac) as SVG path data. Theme tokens only.
(function () {
  const vscode = acquireVsCodeApi();
  const $ = id => document.getElementById(id);
  const SVG = 'http://www.w3.org/2000/svg';
  const main = document.querySelector('main');
  let state = { phase: 'loading' };
  let clock;

  const pad = n => String(n).padStart(2, '0');
  const left = () => Math.max(0, Math.ceil(((state.expiresAt || 0) - Date.now()) / 1000));

  function drawCode() {
    const qr = $('qr');
    qr.replaceChildren();
    if (state.qr) {
      const svg = document.createElementNS(SVG, 'svg');
      svg.setAttribute('viewBox', `0 0 ${state.qr.side} ${state.qr.side}`);
      svg.setAttribute('shape-rendering', 'crispEdges');
      svg.setAttribute('aria-hidden', 'true');
      svg.dataset.side = String(state.qr.side); svg.dataset.quiet = String(state.qr.quiet);
      const paper = document.createElementNS(SVG, 'rect');
      paper.setAttribute('class', 'qr-paper'); paper.setAttribute('width', String(state.qr.side)); paper.setAttribute('height', String(state.qr.side));
      const ink = document.createElementNS(SVG, 'path');
      ink.setAttribute('class', 'qr-ink'); ink.setAttribute('d', state.qr.d);
      svg.append(paper, ink);
      qr.append(svg);
    }
    const code = $('code');
    code.replaceChildren();
    // Groups of four for the eye; the text itself is the code, character for character.
    for (const group of state.groups || []) { const s = document.createElement('span'); s.className = 'group'; s.textContent = group; code.append(s); }
  }

  function tick() {
    const s = left();
    $('left').textContent = state.phase === 'open' ? `Works once, for ${Math.floor(s / 60)}:${pad(s % 60)} more.` : '';
    if (state.phase !== 'open' || s <= 0) { clearInterval(clock); clock = undefined; }
  }

  function render() {
    const phase = state.phase;
    main.dataset.phase = phase;
    $('mac').textContent = state.mac ? `With ${state.mac}` : '';
    const open = phase === 'open';
    $('open').hidden = !open;
    $('typed').hidden = !open;
    $('over').hidden = open || phase === 'loading';
    if (open) drawCode();
    else { $('qr').replaceChildren(); $('code').replaceChildren(); }
    $('over-text').textContent = state.text || '';
    // While the owner decides there is nothing to choose here; after that a new code is one click.
    $('new').hidden = phase === 'waiting';
    $('new').textContent = phase === 'paired' ? 'Pair Another Phone' : 'New code';
    $('new').className = phase === 'paired' ? 'btn' : 'btn primary';
    $('done').hidden = phase !== 'paired';
    $('done').className = phase === 'paired' ? 'btn primary' : 'btn';
    $('copy-label').textContent = 'Copy';
    clearInterval(clock); clock = undefined;
    if (open) { clock = setInterval(tick, 500); }
    tick();
    document.body.dataset.ready = '1';
  }

  $('copy').addEventListener('click', () => vscode.postMessage({ type: 'copy' }));
  $('new').addEventListener('click', () => { $('new').disabled = true; vscode.postMessage({ type: 'new' }); });
  $('done').addEventListener('click', () => vscode.postMessage({ type: 'done' }));
  // Clicking the code selects all of it, ready to copy.
  $('code').addEventListener('click', () => { const r = document.createRange(); r.selectNodeContents($('code')); const sel = getSelection(); sel.removeAllRanges(); sel.addRange(r); });

  window.addEventListener('message', event => {
    const m = event.data;
    if (m?.type === 'state') { state = m.state || { phase: 'loading' }; $('new').disabled = false; render(); }
    else if (m?.type === 'copied') { $('copy-label').textContent = 'Copied'; setTimeout(() => { $('copy-label').textContent = 'Copy'; }, 1500); }
  });
  vscode.postMessage({ type: 'ready' });
})();
