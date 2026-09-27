// Search field for the Overseer side bar: type to filter the Agents list; Escape or ✕ clears.
(function () {
  const vscode = acquireVsCodeApi();
  const q = document.getElementById('q'), count = document.getElementById('count'), clear = document.getElementById('clear');
  let timer;
  const send = () => { clear.hidden = !q.value; vscode.postMessage({ type: 'query', value: q.value.trim() }); };
  q.addEventListener('input', () => { clearTimeout(timer); clear.hidden = !q.value; timer = setTimeout(send, q.value.trim() ? 25 : 150); });
  q.addEventListener('keydown', e => { if (e.key === 'Escape' && q.value) { e.preventDefault(); q.value = ''; count.textContent = ''; send(); } });
  clear.addEventListener('click', () => { q.value = ''; count.textContent = ''; send(); q.focus(); });
  // Filters: one at a time (radio buttons; arrow keys move between them).
  const filters = [...document.querySelectorAll('.filters [role=radio]')];
  const pick = (value, notify) => { for (const b of filters) { const on = b.dataset.filter === value; b.setAttribute('aria-checked', String(on)); b.tabIndex = on ? 0 : -1; } if (notify) vscode.postMessage({ type: 'filter', value }); };
  filters.forEach((b, i) => {
    b.addEventListener('click', () => pick(b.dataset.filter, true));
    b.addEventListener('keydown', e => { const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0; if (!d) return; e.preventDefault(); const n = filters[(i + d + filters.length) % filters.length]; n.focus(); pick(n.dataset.filter, true); });
  });
  pick('all', false);
  window.addEventListener('message', e => {
    const m = e.data || {};
    if (m.type === 'focus') { q.focus(); q.select(); }
    else if (m.type === 'count') count.textContent = m.text || '';
    else if (m.type === 'filter') pick(m.value || 'all', false);
    else if (m.type === 'set') { q.value = m.value || ''; clear.hidden = !q.value; if (!q.value) count.textContent = ''; }
  });
  window.__overseerSearch = { value: () => q.value, count: () => count.textContent, filter: () => filters.find(b => b.getAttribute('aria-checked') === 'true')?.dataset.filter };
})();
