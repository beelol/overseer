// Search field for the Overseer side bar: type to filter the Agents list; Escape or ✕ clears.
(function () {
  const vscode = acquireVsCodeApi();
  const q = document.getElementById('q'), count = document.getElementById('count'), clear = document.getElementById('clear');
  let timer;
  const send = () => { clear.hidden = !q.value; vscode.postMessage({ type: 'query', value: q.value.trim() }); };
  q.addEventListener('input', () => { clearTimeout(timer); clear.hidden = !q.value; timer = setTimeout(send, q.value.trim() ? 25 : 150); });
  q.addEventListener('keydown', e => { if (e.key === 'Escape' && q.value) { e.preventDefault(); q.value = ''; count.textContent = ''; send(); } });
  clear.addEventListener('click', () => { q.value = ''; count.textContent = ''; send(); q.focus(); });
  window.addEventListener('message', e => {
    const m = e.data || {};
    if (m.type === 'focus') { q.focus(); q.select(); }
    else if (m.type === 'count') count.textContent = m.text || '';
    else if (m.type === 'set') { q.value = m.value || ''; clear.hidden = !q.value; if (!q.value) count.textContent = ''; }
  });
  window.__overseerSearch = { value: () => q.value, count: () => count.textContent };
})();
