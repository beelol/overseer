// New-agent composer (AC-59): shown in the middle of the dashboard when no agent is selected.
// Type the task; repository, agent (harness + account), model and workspace are compact chips with
// remembered defaults; Enter starts it. Problems (untrusted workspace, signed-out account, harness
// not installed) appear inline with their fix. The full New Task form stays one click away.
(function () {
  const ui = window.OverseerUI, el = ui.el;
  const MODELS = { claude: ['sonnet', 'opus', 'haiku'], codex: ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'], 'codex-app': ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'], opencode: [] };

  function create(host, { post, onStarted }) {
    let data, form = {}, starting = false, requested = false;
    const wrap = el('div', 'composer-view');
    const hero = el('div', 'composer-hero');
    const mark = el('div', 'hero-mark'); mark.append(ui.icon('eye'));
    const h = el('h1', 'hero-title', 'What should an agent do?');
    const box = el('div', 'composer big');
    const task = el('textarea'); task.id = 'task'; task.rows = 3; task.placeholder = 'Describe the task'; task.setAttribute('aria-label', 'Task for the new agent');
    const generic = el('div', 'generic-fields'); generic.hidden = true;
    const program = el('input'); program.id = 'program'; program.placeholder = '/absolute/path/to/program'; program.setAttribute('aria-label', 'Program to run');
    const args = el('input'); args.id = 'args'; args.placeholder = '["--flag", "value"]'; args.setAttribute('aria-label', 'Arguments as a JSON array');
    generic.append(program, args);
    const row = el('div', 'composer-row'); const chips = el('div', 'chips');
    const repoChip = chip('repo', 'Repository'), agentChip = chip('agent', 'Agent'), modelChip = chip('model', 'Model'), modeChip = chip('mode', 'Workspace');
    chips.append(repoChip, agentChip, modelChip, modeChip);
    const toolsBar = el('div', 'composer-tools');
    const tray = el('div', 'composer-tray'); tray.hidden = true;
    const more = ui.iconButton('ellipsis', 'More options', { cls: 'sm', action: 'composer-more' }); more.setAttribute('aria-haspopup', 'menu');
    const start = ui.iconButton('arrow-up', 'Start agent', { cls: 'primary send', shortcut: 'Enter' }); start.id = 'start';
    row.append(toolsBar, chips, more, start);
    box.append(tray, task, generic, row);
    const tools = window.OverseerPromptTools.create(task, toolsBar, tray, { post, noModel: true, harness: () => form.harness, target: () => form.repo ? { repo: form.repo } : null, notice: t => { note.className = 'composer-note error'; note.replaceChildren(ui.icon('warning', 'sm'), el('span', null, t)); }, onChange: () => {} });
    const note = el('div', 'composer-note'); note.setAttribute('role', 'status');
    const foot = el('div', 'composer-foot');
    const full = el('button', 'link', 'Full form'); full.type = 'button'; full.title = 'Open the New Task form with every option';
    foot.append(el('span', 'kbd-hint', '⏎ start · ⇧⏎ new line'), full);
    hero.append(mark, h, box, note, foot);
    wrap.append(hero);
    host.append(wrap);

    function chip(kind, label) {
      const b = el('button', 'chip'); b.type = 'button'; b.dataset.chip = kind; b.setAttribute('aria-haspopup', 'menu'); b.setAttribute('aria-label', label);
      return b;
    }
    function setChip(b, iconOrLogo, text, title) {
      b.replaceChildren(typeof iconOrLogo === 'string' ? ui.icon(iconOrLogo, 'sm') : iconOrLogo, el('span', 'chip-label', text), ui.icon('chevron-down', 'xs'));
      b.title = title || text; b.setAttribute('aria-label', `${b.getAttribute('aria-label').split(':')[0]}: ${text}`);
    }
    const account = () => data && data.accounts.find(a => a.id === form.account);
    const harness = () => data && data.harnesses.find(x => x.harness === form.harness);

    function render() {
      if (!data) { setChip(repoChip, 'repo', 'Loading…'); return; }
      const repo = data.repos.find(r => r.path === form.repo);
      setChip(repoChip, 'repo', repo ? repo.name : 'Choose repository', repo ? `${repo.path}${repo.branch ? `\nOn ${repo.branch}` : ''}` : 'Choose a Git repository');
      const hx = harness(), a = account();
      const agentLabel = form.harness === 'generic' ? 'Program' : `${ui.HARNESS[form.harness] || form.harness || 'Agent'}${a ? ' · ' + a.name : ''}`;
      setChip(agentChip, ui.harnessMark(form.harness, 14), agentLabel, [hx && `${ui.HARNESS[hx.harness]} ${hx.version || ''}`, a && `${a.name}: ${a.signedIn ? 'signed in' + (a.plan ? ' · ' + a.plan : '') : 'not signed in'}`, a && ui.usageDetail(a.usage)].filter(Boolean).join('\n'));
      modelChip.hidden = form.harness === 'generic';
      setChip(modelChip, 'sparkle', form.model || 'Default model', form.model ? `Model: ${form.model}` : 'The harness default model');
      setChip(modeChip, form.mode === 'current' ? 'repo' : 'git-branch', form.mode === 'current' ? 'Current checkout' : 'New worktree', form.mode === 'current' ? 'Works directly in your checkout' : `A new branch and worktree${form.ref ? ' from ' + form.ref : ''}; your checkout is untouched`);
      generic.hidden = form.harness !== 'generic';
      tools.refresh();
      task.placeholder = form.harness === 'generic' ? 'Optional first line for the program' : 'Describe the task';
      validate();
    }
    function problem() {
      if (!data) return {};
      if (!data.trusted) return { text: 'Trust this workspace to start agents.', fix: 'Trust', command: 'workbench.trust.manage' };
      if (!form.repo) return { text: 'Choose a repository.', fix: 'Choose…', action: () => post({ type: 'composerBrowse' }) };
      const hx = harness();
      if (!hx) return { text: 'Choose an agent.' };
      if (!hx.installed) return { text: `${ui.HARNESS[hx.harness] || hx.harness} is not installed.`, fix: 'How to install', url: hx.install_url };
      if (form.harness !== 'generic') {
        const a = account();
        if (!a) return { text: 'Add an account for this agent.', fix: 'Add account', command: 'overseer.addProfile' };
        if (!a.signedIn) return { text: `${a.name} is not signed in.`, fix: 'Sign in', command: 'overseer.signIn', args: { profile: { id: a.id, name: a.name } } };
        const near = ui.nearLimit(a.usage);
        if (near && !form.limitAcknowledged) {
          const better = data.accounts.filter(x => x.id !== a.id && x.signedIn && (x.harnesses || []).includes(form.harness)).sort((x, y) => (ui.nearLimit(x.usage, 0)?.used || 0) - (ui.nearLimit(y.usage, 0)?.used || 0)).find(x => !ui.nearLimit(x.usage));
          return { warn: true, text: `${a.name} is at ${Math.round(near.used * 100)}% of its ${near.label} limit${near.resets_at_ms ? ' (resets ' + new Date(near.resets_at_ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }) + ')' : ''}.`,
            fix: better ? `Use ${better.name}` : 'Start anyway', action: () => { if (better) form.account = better.id; else form.limitAcknowledged = true; save(); } };
        }
      } else if (!/^\//.test(program.value.trim())) return { text: 'Enter the absolute path of the program.', soft: true };
      if (form.harness !== 'generic' && !task.value.trim()) return { soft: true };
      return {};
    }
    function validate() {
      const p = problem();
      note.replaceChildren(); note.className = 'composer-note' + (p.text && !p.soft && !p.warn ? ' error' : p.warn ? ' warn' : '');
      if (p.text) {
        note.append(ui.icon(p.soft ? 'info' : 'warning', 'sm'), el('span', null, p.text));
        if (p.fix) { const b = el('button', 'link fix', p.fix); b.type = 'button'; b.addEventListener('click', () => { if (p.action) p.action(); else if (p.url) post({ type: 'openExternal', url: p.url }); else post({ type: 'command', command: p.command, args: p.args }); }); note.append(b); }
      }
      start.disabled = (!!p.text && !p.warn) || !!(p.soft) || starting;
      start.title = p.text || (p.soft ? 'Describe the task first' : 'Start agent (Enter)');
      return (!p.text || p.warn) && !p.soft;
    }

    function menuRepo() {
      ui.menu(repoChip, [...data.repos.map(r => ({ label: r.name, icon: 'repo', hint: r.source === 'open folder' ? 'open' : '', checked: r.path === form.repo, title: r.path, run: () => { form.repo = r.path; save(); } })),
        'sep', { label: 'Choose folder…', icon: 'folder-opened', run: () => post({ type: 'composerBrowse' }) }], { label: 'Repository' });
    }
    function menuAgent() {
      const items = [];
      for (const hx of data.harnesses) {
        if (hx.harness === 'codex-app' && !data.showAppServer) continue;
        items.push({ head: `${ui.HARNESS[hx.harness] || hx.harness}${hx.installed ? '' : ' · not installed'}` });
        if (hx.harness === 'generic') { items.push({ label: 'Run a program', logo: ui.harnessMark('generic', 14), checked: form.harness === 'generic', run: () => { form.harness = 'generic'; save(); } }); continue; }
        const accts = data.accounts.filter(a => (a.harnesses || []).includes(hx.harness));
        if (!accts.length) items.push({ label: 'Add account…', icon: 'person-add', run: () => post({ type: 'command', command: 'overseer.addProfile' }) });
        for (const a of accts) items.push({ label: a.name, logo: ui.harnessMark(hx.harness, 14), hint: a.signedIn ? (ui.usageText(a.usage) || a.plan || '') : 'signed out', checked: form.harness === hx.harness && form.account === a.id,
          title: `${a.name}: ${a.signedIn ? 'signed in' : 'not signed in'}${a.kind === 'follows-app' ? ' · follows the desktop app' : ''}`, run: () => { form.harness = hx.harness; form.account = a.id; if (!MODELS[hx.harness]?.includes(form.model)) form.model = ''; save(); } });
      }
      ui.menu(agentChip, items, { label: 'Agent' });
    }
    function menuModel() {
      const models = MODELS[form.harness] || [];
      ui.menu(modelChip, [{ label: 'Default model', icon: 'sparkle', checked: !form.model, run: () => { form.model = ''; save(); } }, ...models.map(m => ({ label: m, icon: 'sparkle', checked: form.model === m, run: () => { form.model = m; save(); } })),
        'sep', { label: 'Other model…', icon: 'edit', run: () => post({ type: 'composerModel', harness: form.harness, current: form.model }) }], { label: 'Model' });
    }
    function menuMode() {
      const items = [{ label: 'New worktree', icon: 'git-branch', checked: form.mode !== 'current', title: 'Isolated branch and worktree; your checkout is untouched', run: () => { form.mode = 'worktree'; save(); } },
        { label: 'Current checkout', icon: 'repo', checked: form.mode === 'current', title: 'Works directly in your checkout; your work is preserved', run: () => { form.mode = 'current'; save(); } }];
      if (form.mode !== 'current' && data.branches && data.branches.repo === form.repo) {
        items.push('sep', { head: 'Start from' }, { label: `HEAD${data.branches.head ? ' (' + data.branches.head + ')' : ''}`, icon: 'git-commit', checked: !form.ref, run: () => { form.ref = ''; save(); } },
          ...data.branches.list.slice(0, 12).map(b => ({ label: b, icon: 'git-branch', checked: form.ref === b, run: () => { form.ref = b; save(); } })));
      }
      ui.menu(modeChip, items, { label: 'Workspace' });
    }
    function menuMore() {
      const items = [{ label: 'Full New Task form', icon: 'window', run: () => post({ type: 'command', command: 'overseer.newTask' }) }];
      if (form.harness === 'codex-app') for (const p of ['on-request', 'untrusted', 'never']) items.push({ label: `Approvals: ${p}`, icon: 'shield', checked: (form.approval || 'on-request') === p, run: () => { form.approval = p; save(); } });
      ui.menu(more, items, { label: 'More options' });
    }
    repoChip.addEventListener('click', () => data && menuRepo());
    agentChip.addEventListener('click', () => data && menuAgent());
    modelChip.addEventListener('click', () => data && menuModel());
    modeChip.addEventListener('click', () => { if (!data) return; if (form.repo && (!data.branches || data.branches.repo !== form.repo)) post({ type: 'composerBranches', repo: form.repo }); menuMode(); });
    more.addEventListener('click', () => data && menuMore());
    full.addEventListener('click', () => post({ type: 'command', command: 'overseer.newTask' }));
    const grow = () => { task.style.height = 'auto'; task.style.height = Math.min(320, Math.max(66, task.scrollHeight)) + 'px'; };
    task.addEventListener('input', () => { grow(); validate(); });
    program.addEventListener('input', validate);
    task.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); go(); } });
    start.addEventListener('click', go);

    function save() { post({ type: 'composerDefaults', defaults: { repo: form.repo, harness: form.harness, account: form.account, model: form.model, mode: form.mode, approval: form.approval } }); render(); }
    function go() {
      if (!validate() || starting) return;
      starting = true; start.disabled = true; note.className = 'composer-note'; note.replaceChildren(el('span', 'mini-dot'), el('span', null, 'Starting…'));
      const { prompt, options } = tools.take();
      post({ type: 'start', form: { ...form, prompt, options: { effort: options.effort, permission_mode: options.permission_mode, images: options.images }, program: program.value.trim(), args: args.value.trim() || '[]' } });
    }
    return {
      open(opts = {}) {
        if (!requested || opts.refresh) { requested = true; post({ type: 'composerData' }); }
        if (opts.repo) { form.repo = opts.repo; render(); }
        setTimeout(() => task.focus(), 0);
      },
      data(d) {
        data = d;
        const def = d.defaults || {};
        form = { repo: form.repo || def.repo || d.repos[0]?.path, harness: form.harness || def.harness, account: form.account || def.account, model: form.model ?? def.model ?? '', mode: form.mode || def.mode || 'worktree', approval: form.approval || def.approval, ref: form.ref || '' };
        if (form.repo && !d.repos.some(r => r.path === form.repo)) form.repo = d.repos[0]?.path;
        // No remembered agent: prefer an installed harness with a signed-in account.
        if (!d.harnesses.some(h => h.harness === form.harness && h.installed)) {
          const usable = d.harnesses.filter(h => h.installed && h.harness !== 'generic' && h.harness !== 'codex-app');
          form.harness = (usable.find(h => d.accounts.some(a => a.signedIn && (a.harnesses || []).includes(h.harness))) || usable[0] || {}).harness;
        }
        const compatible = d.accounts.filter(a => (a.harnesses || []).includes(form.harness));
        if (!compatible.some(a => a.id === form.account)) form.account = (compatible.find(a => a.signedIn) || compatible[0] || {}).id;
        if (d.branches) data.branches = d.branches;
        render(); grow();
      },
      notice(m) {
        if (m.kind === 'started') { starting = false; task.value = ''; program.value = ''; grow(); render(); onStarted(m.runId); return; }
        if (m.kind === 'repo') { data.repos.unshift(m.repo); form.repo = m.repo.path; save(); return; }
        if (m.kind === 'branches') { data.branches = m.branches; menuMode(); return; }
        if (m.kind === 'model') { form.model = m.model; save(); return; }
        starting = false; validate();
        note.className = 'composer-note error'; note.replaceChildren(ui.icon('error', 'sm'), el('span', null, m.message));
      },
      onState(s) { if (data && s.accounts) { data.accounts = s.accounts; render(); } },
      mentionFiles(m) { tools.files(m); },
    };
  }
  window.OverseerComposer = { create };
})();
