// All bundle and story text is built as text nodes. No mod supplies HTML or scripts.
(function () {
  const vscode = acquireVsCodeApi(), ui = window.OverseerUI, el = ui.el, root = document.getElementById('mods');
  let state, busy = false;
  const post = message => { if (!message) return; if (message.action === 'link') { vscode.postMessage(message); return; } busy = true; root.setAttribute('aria-busy', 'true'); root.querySelectorAll('button[data-mutates]').forEach(b => { b.disabled = true; }); vscode.postMessage(message); };
  const line = (text, cls = 'mod-note') => el('p', cls, text);
  function button(label, message, mutate = false, primary = false) {
    const b = el('button', 'btn' + (primary ? ' primary' : ''), label); b.type = 'button';
    if (typeof message !== 'function') b.id = ['mod', message.action, message.bindingId || message.fingerprint || message.previewId || message.source || 'library', label].join('-').replace(/[^a-zA-Z0-9_-]/g, '-');
    if (mutate) { b.dataset.mutates = 'true'; b.disabled = busy || !state.trusted || !state.connected; }
    b.addEventListener('click', () => post(typeof message === 'function' ? message() : message)); return b;
  }
  function field(label, input) { const row = el('label', 'mod-field'); row.append(el('span', null, label), input); return row; }
  function select(options, selected) { const n = el('select'); for (const [value, label] of options) { const o = el('option', null, label); o.value = value; o.selected = value === selected; n.append(o); } return n; }
  function check(label, checked = false) { const n = el('input'); n.type = 'checkbox'; n.checked = checked; const f = field(label, n); f.classList.add('mod-check'); return { node: f, input: n }; }
  function section(title) { const n = el('section', 'mod-section'); n.append(el('h2', null, title)); return n; }
  function details(label, body) { const d = el('details'); d.append(el('summary', null, label), body); return d; }
  function facts(pairs) { const dl = el('dl', 'mod-facts'); for (const [k, v] of pairs) { dl.append(el('dt', null, k), el('dd', null, String(v ?? 'Unknown'))); } return dl; }
  function bindingForm(version) {
    const form = el('form', 'mod-binding-form'); form.dataset.fingerprint = version.fingerprint; form.setAttribute('aria-label', `Enable ${version.name} for a scope`);
    const scope = select([['all_agents', 'All agents (excludes Overseer and watchers)'], ['repository', 'Repository'], ['watchers', 'Watchers'], ['agent', 'One agent'], ['overseer', 'Overseer']], 'all_agents');
    const repo = select([...new Set([state.repoKey, ...(state.repositories || [])].filter(Boolean))].map(k => [k, k]));
    const run = select((state.runs || []).map(r => [r.id, r.title]), state.runId);
    const repoField = field('Repository identity', repo), runField = field('Agent', run);
    const update = () => { repoField.hidden = scope.value !== 'repository'; runField.hidden = scope.value !== 'agent'; }; scope.addEventListener('change', update); update();
    form.append(field('Scope', scope), repoField, runField);
    const required = check('Require delivery before launching'), locked = check('Lock this binding against narrower overrides'); form.append(required.node, locked.node);
    const filter = details('Exact harness, account and model filters', el('div', 'mod-filters'));
    const inputs = {};
    for (const [k, label] of [['harnesses', 'Harness identifiers'], ['accounts', 'Shared account pool identifiers'], ['models', 'Model identifiers']]) { const input = el('input'); input.type = 'text'; input.placeholder = 'Any (empty)'; input.disabled = !state.trusted; inputs[k] = input; filter.lastChild.append(field(label + ' (one per comma)', input)); }
    filter.lastChild.append(line('Filters match exact identifiers. Local model selection remains unsupported.')); form.append(filter);
    const actions = el('div', 'mod-actions');
    const make = enabled => {
      const s = { kind: scope.value }; if (s.kind === 'repository') s.repo_key = repo.value; if (s.kind === 'agent') s.run_id = run.value;
      if ((s.kind === 'repository' && !s.repo_key) || (s.kind === 'agent' && !s.run_id)) { scope.focus(); return null; }
      return { action: 'bind', revision: state.data.revision, fingerprint: version.fingerprint, scope: s, enabled, required: required.input.checked, locked: locked.input.checked, filters: Object.fromEntries(Object.entries(inputs).map(([k, i]) => [k, i.value.split(',').map(s => s.trim()).filter(Boolean)])) };
    };
    const action = (label, enabled) => { const b = button(label, () => make(enabled), true, enabled); b.addEventListener('click', e => e.preventDefault()); return b; };
    actions.append(action('Enable for scope', true), action('Disable for scope', false)); for (const b of actions.querySelectorAll('button')) b.id = `mod-scope-${version.fingerprint}-${b.textContent.replace(/ /g, '-')}`; form.append(actions, line('A scoped disable overrides broader settings. Removing the override restores inheritance. Enabling affects future turns; installation alone leaves the mod off.'));
    form.addEventListener('submit', e => e.preventDefault()); if (!state.trusted || !state.connected) form.querySelectorAll('input,select').forEach(i => { i.disabled = true; }); return form;
  }
  function library() {
    const area = section('Library');
    area.append(line('Mods are optional. Preview and install a pinned version, then choose where to enable it.'));
    const top = el('div', 'mod-actions'); top.append(button('Preview local folder…', { action: 'local', operation: 'install' }, true), button('Preview update from folder…', { action: 'local', operation: 'update' }, true)); area.append(top);
    if (!state.library.length) area.append(line('No mods installed. Clear prose is available to preview and remains off until you enable it.'));
    for (const v of state.data.available_bundled || []) {
      const row = el('div', 'mod-row'); row.append(el('h3', null, v.manifest.name), line(v.manifest.summary), button('Preview ' + v.manifest.name, { action: 'preview', source: 'bundled:' + v.id, operation: 'install' }, true)); area.append(row);
    }
    for (const v of state.library) {
      const row = el('div', 'mod-row'); row.dataset.fingerprint = v.fingerprint;
      row.append(el('h3', null, v.name), line(v.state, 'mod-state'), line(v.summary), facts([['Version and fingerprint', v.pin], ['Source', v.source]]));
      if (v.homepage) row.append(button('Open source website', { action: 'link', url: v.homepage }));
      const files = el('ul'); for (const f of v.files || []) files.append(el('li', null, `${f.path} · ${f.bytes} bytes · ${f.sha256}`)); row.append(details('Pinned files', files));
      row.append(details('Set scope', bindingForm(v)));
      const bindings = state.data.bindings.filter(b => b.fingerprint === v.fingerprint);
      for (const b of bindings) {
        const n = el('div', 'mod-binding'); n.append(line(`${scopeName(b.scope)} · ${b.enabled ? 'Enabled' : 'Disabled'}${b.locked ? ' · Locked' : ''}${b.required ? ' · Required' : ''}`, 'mod-state'));
        n.append(line(`Exact filters: ${Object.entries(b.filters || {}).map(([k, vs]) => `${k}: ${vs.length ? vs.join(', ') : 'any'}`).join('; ')}. Owner change: ${b.changed_ms ? new Date(b.changed_ms).toLocaleString() : 'unknown'}.`));
        const actions = el('div', 'mod-actions'); const msg = { revision: state.data.revision, bindingId: b.id, fingerprint: v.fingerprint };
        actions.append(button(b.enabled ? 'Disable binding' : 'Enable binding', { ...msg, action: 'bind', enabled: !b.enabled }, true), button(b.locked ? 'Unlock binding' : 'Lock binding', { ...msg, action: 'bind', enabled: b.enabled, locked: !b.locked }, true), button('Remove override', { ...msg, action: 'unbind' }, true)); n.append(actions); row.append(n);
      }
      row.append(button('Remove pinned version…', { action: 'remove', revision: state.data.revision, fingerprint: v.fingerprint }, true)); area.append(row);
    }
    const planned = el('div', 'mod-row'); planned.append(el('h3', null, 'Less tool noise'), line(state.noise)); area.append(planned); return area;
  }
  const scopeName = s => ({ all_agents: 'All agents (excludes Overseer and watchers)', watchers: 'Watchers', overseer: 'Overseer' })[s.kind] || (s.kind === 'repository' ? 'Repository: ' + s.repo_key : 'One agent: ' + s.run_id);
  function preview() {
    const p = state.preview, area = section('Review preview'); area.id = 'mod-preview';
    area.append(line(`${p.version.manifest.name} · ${p.operation} · version ${p.version.version}`, 'mod-state'), facts([['Fingerprint', p.fingerprint], ['Source', p.version.source], ['Permissions', p.permissions?.length ? JSON.stringify(p.permissions) : 'No executable permissions requested']]));
    const previous = p.previous || []; area.append(line(previous.length ? `Previously installed fingerprints: ${previous.map(v => v.fingerprint).join(', ')}. Existing bindings remain pinned.` : 'This mod has no installed version.'));
    for (const [path, content] of Object.entries(p.contents || {})) {
      const old = previous.flatMap(v => v.files || []).find(f => f.path === path), current = p.files?.find(f => f.path === path);
      const label = `${path} · ${current?.bytes ?? 'unknown'} bytes · ${old ? old.sha256 === current?.sha256 ? 'unchanged' : 'changed' : 'new file'}`;
      const pre = el('pre', 'mod-code', content); area.append(details(label, pre));
    }
    for (const old of previous.flatMap(v => v.files || [])) if (!p.files?.some(f => f.path === old.path)) area.append(line('Removed file: ' + old.path));
    area.append(line(p.notice || 'Installation never enables this mod.'), button(p.operation === 'update' ? 'Update after confirmation…' : 'Install after confirmation…', { action: 'install', previewId: p.id }, true, true)); return area;
  }
  function applied() {
    const a = state.applied, area = section('Applied to a turn'); area.append(field('Inspect agent or Overseer session', select([['', 'Choose a run'], ...(state.runs || []).map(r => [r.id, r.title])], state.runId)));
    area.querySelector('select').id = 'mod-inspect-run';
    area.querySelector('select').addEventListener('change', e => post({ action: 'inspect', runId: e.target.value }));
    area.append(line(a.state, 'mod-state'), facts([['Desired for the next turn', a.desired], ['Last recorded turn', a.last], ['Children', a.coverage]]));
    for (const d of a.decisions) area.append(line(d));
    if (a.snapshot) {
      const s = a.snapshot; area.append(facts([['Turn', s.turn_id], ['Applied fingerprints', (s.applied_fingerprints || []).join(', ') || 'None recorded'], ['Planned fingerprints', (s.planned_fingerprints || []).join(', ') || 'None'], ['Transport', s.transport], ['Original text digest', s.digest]]));
      if (s.text) area.append(details('Recorded mod text' + (s.text_redacted ? ' (credentials redacted)' : ''), el('pre', 'mod-code', s.text)));
    }
    area.append(line(a.notice || 'Text guidance does not enforce prose or change permissions. Disabling does not erase instructions already in session history.')); return area;
  }
  function draw() {
    const focused = document.activeElement, oldId = focused?.id;
    const drafts = new Map([...root.querySelectorAll('form')].map(f => [f.dataset.fingerprint, [...f.querySelectorAll('input,select')].map(i => [i.value, i.checked])])); const opened = [...root.querySelectorAll('details[open]')].map(d => d.querySelector('summary')?.textContent);
    root.replaceChildren(); root.setAttribute('aria-busy', 'false'); root.append(el('h1', null, 'Mods'));
    root.append(button('Refresh', { action: 'refresh' }));
    if (state.error) { const e = line(state.error, 'mod-error'); e.setAttribute('role', 'alert'); root.append(e); }
    if (!state.connected || !state.data) { root.append(line('Current library unavailable. Reconnect and refresh before making changes.')); return; }
    if (!state.trusted) { const n = line('Restricted workspace: Mods are read-only. Trust this workspace to preview, install or change bindings.'); n.setAttribute('role', 'status'); root.append(n); }
    root.append(line(state.qualification), library()); if (state.preview) root.append(preview()); root.append(applied());
    const story = section('Recent mod changes'); story.append(line('Shows the latest retained or observed Mods events; older changes may not be in this page.'));
    for (const ev of state.story || []) { const p = ev.payload || {}; story.append(line(`${ev.ts_ms ? new Date(ev.ts_ms).toLocaleString() : ''} · ${ev.kind === 'mods_applied' ? 'Turn delivery: ' + (p.snapshot?.outcome || 'unknown') : p.operation || 'Changed'} · ${p.mod_id || p.snapshot?.run_id || ''}`)); }
    if (!state.story?.length) story.append(line('No recent mod changes in the retained page.')); root.append(story);
    for (const d of root.querySelectorAll('details')) if (opened.includes(d.querySelector('summary')?.textContent)) d.open = true;
    for (const form of root.querySelectorAll('form')) { const draft = drafts.get(form.dataset.fingerprint); [...form.querySelectorAll('input,select')].forEach((i, n) => { if (draft?.[n]) { i.value = draft[n][0]; if (i.type === 'checkbox') i.checked = draft[n][1]; } i.id = `mod-form-${form.dataset.fingerprint}-${n}`; }); form.querySelector('select')?.dispatchEvent(new Event('change')); }
    [...root.querySelectorAll('button,summary,select')].forEach((n, i) => { if (!n.id) n.id = `mod-control-${i}`; });
    if (oldId) document.getElementById(oldId)?.focus();
  }
  window.addEventListener('message', e => { if (e.data.type !== 'mods') return; state = e.data; busy = false; draw(); });
  vscode.postMessage({ action: 'ready' });
})();
