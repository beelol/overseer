// Search field for the Overseer side bar: type to filter the Agents list; Escape or ✕ clears.
(function () {
  const vscode = acquireVsCodeApi();
  const q = document.getElementById('q'), count = document.getElementById('count'), clear = document.getElementById('clear');
  let timer;
  const send = () => { clear.hidden = !q.value; vscode.postMessage({ type: 'query', value: q.value.trim() }); };
  q.addEventListener('input', () => { clearTimeout(timer); clear.hidden = !q.value; timer = setTimeout(send, q.value.trim() ? 25 : 150); });
  q.addEventListener('keydown', e => { if (e.key === 'Escape' && q.value) { e.preventDefault(); q.value = ''; count.textContent = ''; send(); } });
  clear.addEventListener('click', () => { q.value = ''; count.textContent = ''; send(); q.focus(); });
  // Filters live behind the filter icon (a VS Code menu): the icon fills in while one is on.
  const filterBtn = document.getElementById('filter'); let current = 'all';
  const LABELS = { all: 'All', working: 'Working', needs: 'Needs you', done: 'Done', failed: 'Failed', archived: 'Archived' };
  const pick = value => { current = value; const on = value !== 'all'; filterBtn.classList.toggle('on', on);
    filterBtn.firstElementChild.className = 'codicon codicon-' + (on ? 'filter-filled' : 'filter'); filterBtn.setAttribute('aria-label', 'Filter agents: ' + LABELS[value]); };
  filterBtn.addEventListener('click', () => vscode.postMessage({ type: 'filterMenu' }));
  window.addEventListener('message', e => {
    const m = e.data || {};
    if (m.type === 'focus') { q.focus(); q.select(); }
    else if (m.type === 'count') count.textContent = m.text || '';
    else if (m.type === 'filter') pick(m.value || 'all');
    else if (m.type === 'set') { q.value = m.value || ''; clear.hidden = !q.value; if (!q.value) count.textContent = ''; }
  });
  window.__overseerSearch = { value: () => q.value, count: () => count.textContent, filter: () => current };
})();
