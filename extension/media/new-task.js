// New Task form (AC-47): repository, harness and account are chosen from rounded tiles with
// icons, status and capability hints; each tile group is an ARIA radiogroup (arrow keys move,
// Space/Enter choose, Tab moves between groups). Only compatible accounts are offered for
// the chosen harness (docs/rfcs/account-governance.md). Theme tokens only.
(function () {
  const vscode = acquireVsCodeApi();
  const saved = vscode.getState() || {};
  let data = { repos: [], harnesses: [], accounts: [], branches: {}, trusted: true };
  const form = { repo: saved.repo, harness: saved.harness, account: saved.account, mode: saved.mode || 'worktree', ref: saved.ref || '', model: saved.model || '', approval: saved.approval || 'on-request', prompt: saved.prompt || '', program: saved.program || '', args: saved.args || '[]' };
  const $ = id => document.getElementById(id);
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
  const persist = () => vscode.setState({ ...form });
  const ui = window.OverseerUI;
  const ICON = { repo: 'repo', folder: 'folder-opened', worktree: 'git-branch', current: 'repo', 'on-request': 'shield', untrusted: 'shield', never: 'circle-slash' };

  /** Renders a radiogroup of tiles. items: { value, title, sub, hints[], status, icon, disabled, why } */
  function tiles(container, label, items, value, onPick) {
    container.replaceChildren();
    container.setAttribute('role', 'radiogroup'); container.setAttribute('aria-label', label);
    const enabled = items.filter(i => !i.disabled);
    const current = items.find(i => i.value === value && !i.disabled) || null;
    items.forEach(item => {
      const t = el('div', 'tile' + (item.disabled ? ' disabled' : '') + (current === item ? ' selected' : ''));
      t.setAttribute('role', 'radio'); t.setAttribute('aria-checked', String(current === item)); t.dataset.value = item.value;
      if (item.disabled) t.setAttribute('aria-disabled', 'true');
      t.tabIndex = item.disabled ? -1 : (current ? (current === item ? 0 : -1) : (enabled[0] === item ? 0 : -1));
      const icon = el('span', 'tile-icon'); icon.setAttribute('aria-hidden', 'true');
      icon.append(item.logo || ui.icon(item.icon || 'circle-small', 'sm'));
      const body = el('div', 'tile-body');
      const head = el('div', 'tile-title');
      head.append(el('span', null, item.title));
      if (item.status) { const st = el('span', 'dot ' + item.status.cls); st.title = item.status.text; st.setAttribute('aria-hidden', 'true'); head.append(st); }
      body.append(head);
      if (item.sub) { const sub = el('div', 'tile-sub', item.sub); if (item.subTitle) sub.title = item.subTitle; body.append(sub); }
      if (item.disabled && item.why) body.append(el('div', 'tile-why', item.why));
      if (item.tip) t.title = item.tip;
      t.append(icon, body);
      t.setAttribute('aria-label', [item.title, item.status?.text, item.sub, ...(item.hints || []), item.disabled ? 'unavailable: ' + (item.why || '') : ''].filter(Boolean).join(', '));
      if (!item.disabled) t.addEventListener('click', () => onPick(item.value));
      container.append(t);
    });
    container.onkeydown = e => {
      const list = [...container.querySelectorAll('.tile:not(.disabled)')];
      const i = list.indexOf(document.activeElement);
      if (i < 0) return;
      let next;
      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = list[(i + 1) % list.length];
      else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = list[(i - 1 + list.length) % list.length];
      else if (e.key === ' ' || e.key === 'Enter') { e.preventDefault(); onPick(list[i].dataset.value); container.querySelector(`[data-value="${CSS.escape(list[i].dataset.value)}"]`)?.focus(); return; }
      if (next) { e.preventDefault(); for (const x of list) x.tabIndex = -1; next.tabIndex = 0; next.focus(); onPick(next.dataset.value, true); container.querySelector(`[data-value="${CSS.escape(next.dataset.value)}"]`)?.focus(); }
    };
  }

  function render() {
    // Re-rendering replaces tiles; keep keyboard focus on the same tile (or field).
    const active = document.activeElement;
    const keep = active?.closest?.('[role=radiogroup]') ? { group: active.closest('[role=radiogroup]').id, value: active.dataset.value } : undefined;
    renderAll();
    if (keep) document.getElementById(keep.group)?.querySelector(`[data-value="${CSS.escape(keep.value || '')}"]`)?.focus();
  }
  function renderAll() {
    const repoItems = data.repos.map(r => ({ value: r.path, title: r.name, sub: r.branch || 'detached', subTitle: r.path, tip: `${r.path}${r.branch ? ' · ' + r.branch : ''}`, icon: ICON.repo, hints: [r.branch ? 'on ' + r.branch : 'detached', r.source] }));
    repoItems.push({ value: '__browse__', title: 'Choose…', sub: 'Any Git repository', icon: ICON.folder });
    tiles($('repos'), 'Repository', repoItems, form.repo, (v, moving) => { if (v === '__browse__') { if (!moving) vscode.postMessage({ type: 'browse' }); return; } form.repo = v; persist(); vscode.postMessage({ type: 'branches', repo: v }); render(); });
    tiles($('harnesses'), 'Harness', data.harnesses.map(h => ({ value: h.harness, title: h.label, logo: ui.harnessMark(h.harness, 16), sub: h.installed ? String(h.version || 'installed').split(' ')[0] : 'not installed', tip: [h.label, h.version, ...(h.hints || [])].filter(Boolean).join(' · '),
      status: h.installed ? { cls: 'ok', text: 'ready' } : { cls: 'off', text: 'missing' }, hints: h.hints, disabled: !h.installed, why: h.installed ? '' : 'Not installed' })), form.harness,
      v => { form.harness = v; const ok = compatible().some(a => a.id === form.account); if (!ok) form.account = (compatible().find(a => a.signedIn) || {}).id; persist(); render(); });
    const accounts = compatible();
    $('account-section').hidden = !form.harness || form.harness === 'generic';
    $('generic-section').hidden = form.harness !== 'generic';
    $('approval-section').hidden = form.harness !== 'codex-app';
    tiles($('accounts'), 'Account', accounts.map(a => ({ value: a.id, title: a.name, logo: ui.providerMark(a.provider, 16), sub: [a.plan, a.kind === 'follows-app' ? 'desktop login' : ''].filter(Boolean).join(' · ') || (a.signedIn ? 'signed in' : ''),
      tip: `${a.name}: ${a.signedIn ? 'signed in' : 'not signed in'}${a.plan ? ' · ' + a.plan : ''}${a.fingerprint ? ' · id ' + a.fingerprint : ''}${a.kind === 'follows-app' ? '\nFollows the desktop app login and changes when the app switches accounts' : '\nFixed account with its own credential folder'}`,
      status: a.signedIn ? { cls: 'ok', text: 'signed in' } : { cls: 'warn', text: 'not signed in' }, hints: [a.plan, a.fingerprint && 'id ' + a.fingerprint].filter(Boolean),
      disabled: !a.signedIn, why: a.signedIn ? '' : 'Sign in first' })), form.account, v => { form.account = v; persist(); render(); });
    $('no-accounts').hidden = !form.harness || form.harness === 'generic' || accounts.length > 0;
    tiles($('modes'), 'Workspace', [
      { value: 'worktree', title: 'New worktree', icon: ICON.worktree, sub: 'Recommended', tip: 'Isolated branch and worktree; your checkout is not touched' },
      { value: 'current', title: 'Current checkout', icon: ICON.current, sub: 'Your working tree', tip: 'Works directly in your checkout. Existing staged, unstaged, untracked and unsaved work is recorded and preserved' },
    ], form.mode, v => { form.mode = v; persist(); render(); });
    $('ref-row').hidden = form.mode !== 'worktree';
    const refs = $('ref'); const branches = data.branches[form.repo] || { branches: [], head: '' };
    refs.replaceChildren(el('option', null, `HEAD (${branches.head || 'current'})`));
    refs.options[0].value = '';
    for (const b of branches.branches) { const o = el('option', null, b); o.value = b; refs.append(o); }
    refs.value = branches.branches.includes(form.ref) ? form.ref : '';
    tiles($('approvals'), 'Codex approval policy', [
      { value: 'on-request', title: 'On request', icon: ICON['on-request'], sub: 'Recommended', tip: 'The model asks when it needs to leave the sandbox' },
      { value: 'untrusted', title: 'Untrusted', icon: ICON.untrusted, sub: 'Ask more often', tip: 'Ask before anything that is not a known read-only command' },
      { value: 'never', title: 'Never ask', icon: ICON.never, sub: 'Sandbox only', tip: 'Sandbox limits apply; no approval requests' },
    ], form.approval, v => { form.approval = v; persist(); render(); });
    const ready = !!form.repo && !!form.harness && (form.harness === 'generic' ? !!form.program : !!form.account && !!form.prompt.trim()) && data.trusted;
    $('start').disabled = !ready;
    $('start-why').textContent = !data.trusted ? 'Trust this workspace to start agents' : !form.repo ? 'Choose a repository' : !form.harness ? 'Choose an agent' : form.harness !== 'generic' && !form.account ? 'Choose a signed-in account' : form.harness === 'generic' ? (form.program ? '' : 'Enter the program path') : !form.prompt.trim() ? 'Describe the task' : '';
    document.body.dataset.ready = '1';
  }
  function compatible() { return data.accounts.filter(a => (a.harnesses || []).includes(form.harness)); }

  for (const [id, key] of [['model', 'model'], ['prompt', 'prompt'], ['program', 'program'], ['args', 'args']]) {
    $(id).value = form[key];
    $(id).addEventListener('input', () => { form[key] = $(id).value; persist(); render(); });
  }
  $('ref').addEventListener('change', () => { form.ref = $('ref').value; persist(); });
  $('start').addEventListener('click', () => { if (!$('start').disabled) vscode.postMessage({ type: 'start', form: { ...form } }); });
  $('prompt').addEventListener('keydown', e => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); $('start').click(); } });
  window.addEventListener('message', e => {
    const m = e.data;
    if (m.type === 'data') { data = { ...data, ...m.data }; if (!form.repo && data.repos[0]) { form.repo = data.repos[0].path; vscode.postMessage({ type: 'branches', repo: form.repo }); } render(); }
    else if (m.type === 'branches') { data.branches[m.repo] = m.info; render(); }
    else if (m.type === 'repoAdded') { if (!data.repos.some(r => r.path === m.repo.path)) data.repos.push(m.repo); form.repo = m.repo.path; persist(); vscode.postMessage({ type: 'branches', repo: form.repo }); render(); }
    else if (m.type === 'error') { $('error').textContent = m.message; $('error').hidden = false; }
    else if (m.type === 'started') { form.prompt = ''; $('prompt').value = ''; persist(); }
  });
  vscode.postMessage({ type: 'ready' });
})();
