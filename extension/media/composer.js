// New-agent composer (AC-59): shown in the middle of the dashboard when no agent is selected.
// Type the task; repository, agent (harness + account), model and workspace are compact chips with
// remembered defaults; Enter starts it. Problems (untrusted workspace, signed-out account, harness
// not installed) appear inline with their fix. The full New Task form stays one click away.
(function () {
  const ui = window.OverseerUI, el = ui.el;
  /**
   * The repository picker's fuzzy match (AC-260): 0 when the letters of `q` are not all in `text`
   * in order, otherwise a score that ranks a prefix, then a contiguous run, then letters at word
   * starts, above scattered letters. Case does not matter; an empty query matches everything.
   */
  function fuzzy(q, text) {
    q = String(q || '').toLowerCase(); const t = String(text || '').toLowerCase();
    if (!q) return 1;
    let score = 1, from = 0, last = -2;
    for (const ch of q) {
      const at = t.indexOf(ch, from);
      if (at < 0) return 0;
      if (at === last + 1) score += 2;
      if (at === 0 || /[-_./\s]/.test(t[at - 1])) score += 3;
      last = at; from = at + 1;
    }
    if (t.startsWith(q)) score += 10; else if (t.includes(q)) score += 5;
    return score;
  }
  const MODELS = { claude: ['sonnet', 'opus', 'haiku'], codex: ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'], 'codex-app': ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'], opencode: [] };

  function create(host, { post, onStarted, agents = () => [] }) {
    // AC-236: home talks to Overseer first. `remembered` is the owner's choice (overseer.home.sendTo);
    // New Agent and "Start as an agent" send one task straight to a new agent without changing it.
    let data, form = {}, starting = false, requested = false, remembered = 'overseer', target = 'overseer', explicit = false;
    // AC-259: what was just sent, while the field says so ({ phase: 'sending' | 'started', text }).
    let sent = null, sentTimer = 0;
    // A one-line note from the host ("the grid has nothing to show…") stays until the owner types or sends.
    let info = '';
    const showInfo = () => { note.className = 'composer-note'; note.replaceChildren(ui.icon('info', 'sm'), el('span', null, info)); };
    const PLACEHOLDER = 'Send off a task';
    const TO_OVERSEER = 'Tell Overseer what to do, or ask what is going on';
    const wrap = el('div', 'composer-view');
    const hero = el('div', 'composer-hero');
    // Overseer's mark in full colour (AC-142) and a short question.
    const mark = ui.mark('xl hero-mark', 'Overseer');
    const h = el('h1', 'hero-title', "What's next?");
    // The one-line hint of what to ask (AC-236), while the conversation is empty.
    const hint = el('p', 'composer-hint', 'Ask what your agents are doing, or say what to build: Overseer starts, steers and answers.'); hint.id = 'composer-hint';
    const box = el('div', 'composer big');
    const task = el('textarea'); task.id = 'task'; task.rows = 3; task.placeholder = 'Send off a task'; task.setAttribute('aria-label', 'Task for the new agent');
    const generic = el('div', 'generic-fields'); generic.hidden = true;
    const program = el('input'); program.id = 'program'; program.placeholder = '/absolute/path/to/program'; program.setAttribute('aria-label', 'Program to run');
    const args = el('input'); args.id = 'args'; args.placeholder = '["--flag", "value"]'; args.setAttribute('aria-label', 'Arguments as a JSON array');
    generic.append(program, args);
    // The field holds only the text (with attach, options and Start); the choices sit in a row under it.
    const row = el('div', 'composer-row'); const chips = el('div', 'chips composer-choices'); chips.setAttribute('aria-label', 'Choices for the new agent');
    // AC-182: where Enter sends the text. New agent by default; Overseer by the chip or `@overseer`.
    const targetChip = chip('target', 'Send to'); targetChip.id = 'target';
    const repoChip = chip('repo', 'Repository'), agentChip = chip('agent', 'Agent'), modelChip = chip('model', 'Model'), modeChip = chip('mode', 'Workspace');
    chips.append(repoChip, agentChip, modelChip, modeChip, targetChip);
    const toolsBar = el('div', 'composer-tools');
    const tray = el('div', 'composer-tray'); tray.hidden = true;
    const more = ui.iconButton('ellipsis', 'More options', { cls: 'sm', action: 'composer-more' }); more.setAttribute('aria-haspopup', 'menu');
    const start = ui.iconButton('arrow-up', 'Start agent', { cls: 'primary send', shortcut: 'Enter' }); start.id = 'start';
    row.append(toolsBar, el('span', 'spacer'), more, start);
    box.append(tray, task, generic, row);
    const tools = window.OverseerPromptTools.create(task, toolsBar, tray, { post, noModel: true, harness: () => form.routing === 'auto' ? 'auto' : form.harness, target: () => form.repo ? { repo: form.repo } : null, notice: t => { note.className = 'composer-note error'; note.replaceChildren(ui.icon('warning', 'sm'), el('span', null, t)); }, onChange: () => {} });
    const note = el('div', 'composer-note'); note.setAttribute('role', 'status');
    const foot = el('div', 'composer-foot');
    const full = el('button', 'link', 'Full form'); full.type = 'button'; full.title = 'Open the New Task form with every option';
    foot.append(el('span', 'kbd-hint', '⏎ start · ⇧⏎ new line'), full);
    hero.append(mark, h, hint, box, chips, note, foot);
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

    const toOverseer = () => target === 'overseer' || /^@overseer\b/i.test(task.value.trim());
    function renderTarget() {
      const ov = toOverseer();
      setChip(targetChip, ov ? ui.mark('sm') : 'rocket', ov ? 'Overseer' : 'Start directly', ov ? 'Enter sends this to Overseer, which starts, steers or answers' : 'Enter starts a new agent with this task; @overseer sends it to Overseer instead');
      targetChip.dataset.target = ov ? 'overseer' : 'agent';
      // Starting directly, the chip is its rocket alone (its name is its label and tooltip), so the
      // new-agent view keeps Gate J's text budget (AC-81); the words show for Overseer.
      targetChip.querySelector('.chip-label').hidden = !ov;
      // To Overseer the box needs no choices: Overseer picks where and how an agent runs.
      for (const c of [repoChip, agentChip, modeChip]) c.hidden = ov;
      modelChip.hidden = ov || form.routing === 'auto' || form.harness === 'generic';
      hint.hidden = !ov;
      start.title = ov ? 'Send to Overseer (Enter)' : start.title;
      start.setAttribute('aria-label', ov ? 'Send to Overseer' : 'Start agent');
      task.setAttribute('aria-label', ov ? 'Message to Overseer' : 'Task for the new agent');
      if (!sent) task.placeholder = ov ? TO_OVERSEER : form.routing !== 'auto' && form.harness === 'generic' ? 'Optional first line for the program' : PLACEHOLDER;
      box.dataset.target = ov ? 'overseer' : 'agent';
      foot.firstChild.textContent = ov ? '⏎ send to Overseer · ⇧⏎ new line' : '⏎ start · ⇧⏎ new line';
    }
    /** Where Enter sends: Overseer, or straight to a new agent. `remember` makes it the owner's default. */
    function setTarget(t, { remember = false } = {}) {
      target = t === 'agent' ? 'agent' : 'overseer';
      if (target === 'overseer') task.value = task.value.replace(/^@overseer\s*/i, '');
      if (remember && target !== remembered) { remembered = target; post({ type: 'composerSendTo', target }); }
      render();
    }
    function menuTarget() {
      ui.menu(targetChip, [{ label: 'Overseer', icon: 'comment-discussion', checked: toOverseer(), title: 'Enter sends the message to Overseer, which starts, steers or answers', run: () => setTarget('overseer', { remember: true }) },
        { label: 'Start an agent directly', icon: 'rocket', checked: !toOverseer(), title: 'Enter starts an agent with the task, with the repository, agent, model and workspace choices', run: () => setTarget('agent', { remember: true }) }], { label: 'Send to' });
    }
    // `@` offers the agents by name in a list under the text that never takes the keyboard: typing
    // narrows it, arrows move, Enter or Tab inserts, Escape closes; a named agent reaches Overseer
    // as its id.
    const mentions = el('div', 'agent-mentions'); mentions.hidden = true; mentions.setAttribute('role', 'listbox'); mentions.setAttribute('aria-label', 'Agents'); mentions.id = 'mentions';
    let mentionAt = -1, mentionIndex = 0;
    function mentionPrefix() {
      const caret = task.selectionStart; const before = task.value.slice(0, caret);
      const m = /(^|\s)@([^\s@]*)$/.exec(before);
      return m ? { at: caret - m[2].length - 1, prefix: m[2] } : null;
    }
    function renderMentions() {
      const m = mentionPrefix();
      if (!m) { mentions.hidden = true; mentionAt = -1; return; }
      const q = m.prefix.toLowerCase();
      const items = [{ id: 'overseer', title: 'overseer', harness: '', status: 'Overseer' }, ...agents().slice(0, 12)].filter(a => a.title.toLowerCase().startsWith(q));
      if (!items.length) { mentions.hidden = true; mentionAt = -1; return; }
      mentionAt = m.at; mentionIndex = Math.min(mentionIndex, items.length - 1);
      mentions.replaceChildren(...items.map((a, i) => { const b = el('button', 'agent-mention' + (i === mentionIndex ? ' active' : '')); b.type = 'button'; b.setAttribute('role', 'option'); b.setAttribute('aria-selected', String(i === mentionIndex)); b.dataset.title = a.title;
        b.append(a.harness ? ui.harnessMark(a.harness, 14) : ui.mark('sm'), el('span', 'agent-mention-title', a.title), el('span', 'agent-mention-hint', a.status || '')); b.addEventListener('mousedown', e => { e.preventDefault(); insertMention(a.title); }); return b; }));
      mentions.hidden = false;
    }
    function insertMention(title) {
      if (mentionAt < 0) return;
      const v = task.value; const caret = task.selectionStart;
      const text = `@${title} `;
      task.value = v.slice(0, mentionAt) + text + v.slice(caret); task.focus(); task.selectionStart = task.selectionEnd = mentionAt + text.length;
      mentions.hidden = true; mentionAt = -1; mentionIndex = 0; grow(); render();
    }
    /** The text as Overseer receives it: agent names as ids. */
    function forOverseer(text) {
      let out = text.replace(/^@overseer\s*/i, '');
      for (const a of agents()) out = out.split('@' + a.title).join(`@${a.title} (${a.id})`);
      return out;
    }
    function render() {
      renderTarget();
      if (!data) { setChip(repoChip, 'repo', 'Loading…'); return; }
      const repo = data.repos.find(r => r.path === form.repo);
      setChip(repoChip, 'repo', repo ? repo.name : 'Choose repository', repo ? `${repo.path}${repo.branch ? `\nOn ${repo.branch}` : ''}` : 'Choose a Git repository');
      const hx = harness(), a = account();
      const agentLabel = form.routing === 'auto' ? `Auto routing${form.preferredHarness ? ' · prefer ' + (ui.HARNESS[form.preferredHarness] || form.preferredHarness) : ''}` : form.harness === 'generic' ? 'Program' : `${ui.HARNESS[form.harness] || form.harness || 'Agent'}${a ? ' · ' + a.name : ''}`;
      setChip(agentChip, form.routing === 'auto' ? 'sparkle' : ui.harnessMark(form.harness, 14), agentLabel, form.routing === 'auto' ? 'Selects an eligible account, agent, model and effort for each work unit' : [hx && `${ui.HARNESS[hx.harness]} ${hx.version || ''}`, a && `${a.name}: ${a.signedIn ? 'signed in' + (a.plan ? ' · ' + a.plan : '') : 'not signed in'}`, a && ui.usageDetail(a.usage)].filter(Boolean).join('\n'));
      modelChip.hidden = toOverseer() || form.routing === 'auto' || form.harness === 'generic';
      setChip(modelChip, 'sparkle', form.model || 'Default model', form.model ? `Model: ${form.model}` : 'The harness default model');
      setChip(modeChip, form.mode === 'current' ? 'repo' : 'git-branch', form.mode === 'current' ? 'Current checkout' : 'New worktree', form.mode === 'current' ? 'Works directly in your checkout' : `A new branch and worktree${form.ref ? ' from ' + form.ref : ''}; your checkout is untouched`);
      generic.hidden = toOverseer() || form.routing === 'auto' || form.harness !== 'generic';
      // Continuity (Gate L): a local agent's chips, and the offline line above the field.
      if (window.OverseerContinuity) window.OverseerContinuity.chips({ data, form: form.routing === 'auto' ? { ...form, harness: '' } : form, agentChip, modelChip, setChip });
      tools.refresh();
      task.placeholder = sent ? sentPlaceholder() : toOverseer() ? TO_OVERSEER : form.routing !== 'auto' && form.harness === 'generic' ? 'Optional first line for the program' : PLACEHOLDER;
      validate();
    }
    function problem() {
      if (!data) return {};
      const continuity = form.routing !== 'auto' && window.OverseerContinuity && window.OverseerContinuity.problem({ data, form, save, task: task.value });
      if (continuity) return continuity;
      if (!data.trusted) return { text: 'Trust this workspace to start agents.', fix: 'Trust', command: 'workbench.trust.manage' };
      if (!form.repo) return { text: 'Choose a repository.', fix: 'Choose…', action: () => openRepoPicker() };
      if (form.routing === 'auto') {
        const supported = data.accounts.filter(a => a.id !== 'local-ollama' && ((a.signedIn && (a.harnesses || []).some(h => ['codex', 'claude'].includes(h))) || (a.installed && (a.harnesses || []).includes('opencode'))));
        if (!supported.length) return { text: 'Connect Codex or Claude Code, or set up a project-local OpenCode route for Auto routing.', fix: 'Add account', command: 'overseer.addProfile' };
        if (supported.length > 8) return { text: 'Auto routing supports up to eight signed-in accounts.' };
        if (!task.value.trim()) return { soft: true };
        return {};
      }
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
      if (toOverseer()) { if (info) showInfo(); else { note.replaceChildren(); note.className = 'composer-note'; } start.disabled = !task.value.trim(); return true; }
      const p = problem();
      if (sent && (!p.text || p.soft || p.warn)) { showSentNote(); start.disabled = true; start.title = 'Describe the next task'; return false; }
      note.replaceChildren(); note.className = 'composer-note' + (p.text && !p.soft && !p.warn ? ' error' : p.warn ? ' warn' : '');
      if (info && (!p.text || p.soft)) showInfo();
      if (p.text) {
        note.append(ui.icon(p.soft ? 'info' : 'warning', 'sm'), el('span', null, p.text));
        if (p.fix) { const b = el('button', 'link fix', p.fix); b.type = 'button'; b.addEventListener('click', () => { if (p.action) p.action(); else if (p.url) post({ type: 'openExternal', url: p.url }); else post({ type: 'command', command: p.command, args: p.args }); }); note.append(b); }
      }
      start.disabled = (!!p.text && !p.warn) || !!(p.soft) || starting;
      start.title = p.text || (p.soft ? 'Describe the task first' : 'Start agent (Enter)');
      return (!p.text || p.warn) && !p.soft;
    }

    // The repository chip's own picker (AC-260), inside the webview: recent repositories, a fuzzy
    // search over every known one (open, recent, and Git repositories beside an open one), and a
    // typed path (`/…` or `~/…`, Tab completes a folder). The system folder dialog is its last
    // option, never the only way. The keyboard stays in its search field throughout.
    let picker = null, hintTimer = 0;
    const isPath = q => /^(~$|~\/|\/)/.test(q);
    function openRepoPicker() {
      if (picker) { closeRepoPicker(); return; }
      if (!data) return;
      ui.closeMenu();
      const m = el('div', 'menu repo-picker'); m.setAttribute('role', 'dialog'); m.setAttribute('aria-label', 'Repository');
      const input = el('input', 'repo-picker-input'); input.id = 'repo-query'; input.type = 'text'; input.spellcheck = false; input.autocomplete = 'off';
      input.placeholder = 'Search repositories, or type a path'; input.setAttribute('aria-label', 'Search repositories, or type a path (/ or ~)');
      input.setAttribute('role', 'combobox'); input.setAttribute('aria-expanded', 'true'); input.setAttribute('aria-controls', 'repo-options'); input.setAttribute('aria-autocomplete', 'list');
      const list = el('div', 'repo-picker-list'); list.id = 'repo-options'; list.setAttribute('role', 'listbox'); list.setAttribute('aria-label', 'Repositories');
      const status = el('div', 'repo-picker-status'); status.setAttribute('role', 'status');
      const foot = el('div', 'repo-picker-foot', '↑↓ choose · ⏎ use · ⇥ complete a path · esc close');
      m.append(input, status, list, foot);
      document.body.append(m);
      picker = { el: m, input, list, status, active: 0, items: [], hints: [], hintsFor: '', busy: '', error: '' };
      repoChip.setAttribute('aria-expanded', 'true');
      input.addEventListener('input', () => { picker.active = 0; picker.error = ''; requestHints(); renderPicker(); });
      input.addEventListener('keydown', pickerKey);
      renderPicker();
      input.focus();
    }
    function closeRepoPicker({ focus = 'chip' } = {}) {
      if (!picker) return;
      picker.el.remove(); picker = null; clearTimeout(hintTimer);
      repoChip.setAttribute('aria-expanded', 'false');
      if (focus === 'task') task.focus(); else if (focus === 'chip') repoChip.focus();
    }
    function pickerItems() {
      const q = picker.input.value.trim();
      const items = [];
      if (isPath(q)) {
        items.push({ kind: 'path', path: q, label: `Use ${q}`, icon: 'folder', hint: 'add' });
        if (picker.hintsFor === q) for (const h of picker.hints.filter(x => x.path !== q && x.path !== q + '/')) items.push({ kind: 'hint', path: h.path, label: h.path, icon: h.git ? 'repo' : 'folder', hint: h.git ? 'Git' : '', git: h.git });
      } else {
        // Nearby repositories come up only when searched for; with no search, open and recent ones.
        const pool = data.repos.filter(r => q || r.source !== 'nearby');
        const scored = pool.map((r, i) => ({ r, i, s: Math.max(fuzzy(q, r.name) * 2, fuzzy(q, r.path)) })).filter(x => x.s > 0);
        if (q) scored.sort((a, b) => b.s - a.s || a.i - b.i);
        for (const { r } of scored.slice(0, 30)) items.push({ kind: 'repo', path: r.path, label: r.name, detail: r.path, icon: 'repo', checked: r.path === form.repo,
          hint: r.source === 'open folder' ? 'open' : r.source === 'nearby' ? 'nearby' : 'recent' });
      }
      items.push({ kind: 'browse', label: 'Browse with the system dialog…', icon: 'folder-opened' });
      return items;
    }
    function renderPicker() {
      if (!picker) return;
      const q = picker.input.value.trim();
      const items = picker.items = pickerItems();
      picker.active = Math.max(0, Math.min(picker.active, items.length - 1));
      picker.list.replaceChildren(...items.map((it, i) => {
        const o = el('div', 'menu-item repo-option' + (i === picker.active ? ' active' : '')); o.id = `repo-option-${i}`;
        o.setAttribute('role', 'option'); o.setAttribute('aria-selected', String(i === picker.active));
        if (it.kind === 'hint') o.dataset.hint = it.path;
        o.dataset.kind = it.kind; if (it.path) o.title = it.path;
        o.append(ui.icon(it.checked ? 'check' : it.icon, 'sm menu-check'), el('span', 'menu-label', it.label));
        if (it.detail) o.append(el('span', 'repo-option-path', it.detail.replace(/\/[^/]+$/, '')));
        if (it.hint) o.append(el('span', 'menu-hint', it.hint));
        o.addEventListener('mousedown', e => { e.preventDefault(); picker.active = i; choose(it); });
        return o;
      }));
      picker.input.setAttribute('aria-activedescendant', `repo-option-${picker.active}`);
      picker.list.querySelector('.active')?.scrollIntoView({ block: 'nearest' });
      picker.status.className = 'repo-picker-status' + (picker.error ? ' repo-picker-error' : '');
      const onlyBrowse = items.length === 1;
      picker.status.replaceChildren(...(picker.error ? [ui.icon('warning', 'sm'), el('span', null, picker.error)]
        : picker.busy ? [el('span', 'mini-dot'), el('span', null, `Adding ${picker.busy}…`)]
        : onlyBrowse && q ? [ui.icon('info', 'sm'), el('span', null, 'No known repository matches. Type a path starting with / or ~.')] : []));
      picker.status.hidden = !picker.status.childNodes.length;
      placePicker();
    }
    function placePicker() {
      const m = picker.el, r = repoChip.getBoundingClientRect();
      const width = Math.min(Math.max(r.width, 380), innerWidth - 16);
      m.style.width = width + 'px';
      const h = m.getBoundingClientRect().height;
      m.style.left = Math.max(8, Math.min(r.left, innerWidth - width - 8)) + 'px';
      m.style.top = (r.bottom + 4 + h > innerHeight - 8 ? Math.max(8, r.top - h - 4) : r.bottom + 4) + 'px';
    }
    function requestHints() {
      clearTimeout(hintTimer);
      const q = picker.input.value.trim();
      if (!isPath(q)) return;
      hintTimer = setTimeout(() => post({ type: 'composerPathHints', input: q }), 60);
    }
    function addPath(p) { picker.busy = p; picker.error = ''; renderPicker(); post({ type: 'composerAddRepo', path: p }); }
    function complete(to) { picker.input.value = to; picker.active = 0; picker.error = ''; requestHints(); renderPicker(); }
    function choose(it) {
      if (!it || picker.busy) return;
      if (it.kind === 'repo') { form.repo = it.path; closeRepoPicker({ focus: 'task' }); save(); return; }
      if (it.kind === 'path') { addPath(it.path); return; }
      if (it.kind === 'hint') { if (it.git) addPath(it.path); else complete(it.path); return; }
      if (it.kind === 'browse') { closeRepoPicker(); post({ type: 'composerBrowse' }); }
    }
    function pickerKey(e) {
      const n = picker.items.length;
      if (e.key === 'ArrowDown') { picker.active = (picker.active + 1) % n; renderPicker(); }
      else if (e.key === 'ArrowUp') { picker.active = (picker.active - 1 + n) % n; renderPicker(); }
      else if (e.key === 'Enter') choose(picker.items[picker.active]);
      else if (e.key === 'Escape') closeRepoPicker();
      else if (e.key === 'Tab' && !e.shiftKey && isPath(picker.input.value.trim())) {
        const act = picker.items[picker.active];
        const to = act && act.kind === 'hint' ? act : picker.items.find(x => x.kind === 'hint');
        if (to) complete(to.path);
      } else if (e.key === 'Tab') closeRepoPicker();
      else return;
      e.preventDefault(); e.stopPropagation();
    }
    document.addEventListener('mousedown', e => { if (picker && !picker.el.contains(e.target) && !repoChip.contains(e.target)) closeRepoPicker({ focus: 'none' }); });
    window.addEventListener('blur', () => closeRepoPicker({ focus: 'none' }));
    window.addEventListener('resize', () => picker && placePicker());
    function menuAgent() {
      // Auto routing is unfinished in this build: offered only when its setting is on (AC-204).
      const items = !data.autoRouting ? [] : [{ head: 'Automatic selection' },
        { label: 'Auto routing · any eligible agent', icon: 'sparkle', checked: form.routing === 'auto' && !form.preferredHarness,
          run: () => { form.routing = 'auto'; form.preferredHarness = ''; save(); } },
        ...[['codex-app', 'Codex'], ['claude', 'Claude Code'], ['opencode', 'OpenCode']].map(([value, label]) =>
          ({ label: `Auto routing · prefer ${label}`, icon: 'sparkle', checked: form.routing === 'auto' && form.preferredHarness === value,
            run: () => { form.routing = 'auto'; form.preferredHarness = value; save(); } })), 'sep'];
      for (const hx of data.harnesses) {
        if (hx.harness === 'codex-app' && !data.showAppServer) continue;
        items.push({ head: `${ui.HARNESS[hx.harness] || hx.harness}${hx.installed ? '' : ' · not installed'}` });
        if (hx.harness === 'generic') { items.push({ label: 'Run a program', logo: ui.harnessMark('generic', 14), checked: form.routing !== 'auto' && form.harness === 'generic', run: () => { form.routing = 'manual'; form.harness = 'generic'; save(); } }); continue; }
        const accts = data.accounts.filter(a => (a.harnesses || []).includes(hx.harness));
        if (!accts.length) items.push({ label: 'Add account…', icon: 'person-add', run: () => post({ type: 'command', command: 'overseer.addProfile' }) });
        for (const a of accts) items.push({ label: a.name, logo: ui.harnessMark(hx.harness, 14), hint: a.signedIn ? (ui.usageText(a.usage) || a.plan || '') : 'signed out', checked: form.routing !== 'auto' && form.harness === hx.harness && form.account === a.id,
          title: `${a.name}: ${a.signedIn ? 'signed in' : 'not signed in'}${a.kind === 'follows-app' ? ' · follows the desktop app' : ''}`, run: () => { form.routing = 'manual'; form.harness = hx.harness; form.account = a.id; if (!MODELS[hx.harness]?.includes(form.model)) form.model = ''; save(); } });
      }
      if (window.OverseerContinuity) window.OverseerContinuity.agentMenu(items, { data, form, save });
      ui.menu(agentChip, items, { label: 'Agent' });
    }
    function menuModel() {
      if (window.OverseerContinuity && window.OverseerContinuity.modelMenu(modelChip, { data, form, save })) return;
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
    targetChip.addEventListener('click', menuTarget);
    box.append(mentions);
    task.addEventListener('keydown', e => {
      if (mentions.hidden) return;
      const n = mentions.children.length;
      if (e.key === 'ArrowDown') { mentionIndex = (mentionIndex + 1) % n; renderMentions(); e.preventDefault(); }
      else if (e.key === 'ArrowUp') { mentionIndex = (mentionIndex + n - 1) % n; renderMentions(); e.preventDefault(); }
      else if (e.key === 'Enter' || e.key === 'Tab') { const b = mentions.children[mentionIndex]; if (b) { insertMention(b.dataset.title); e.preventDefault(); e.stopImmediatePropagation(); } }
      else if (e.key === 'Escape') { mentions.hidden = true; mentionAt = -1; e.preventDefault(); e.stopImmediatePropagation(); }
    }, true);
    task.addEventListener('blur', () => setTimeout(() => { mentions.hidden = true; }, 150));
    repoChip.addEventListener('click', () => openRepoPicker());
    agentChip.addEventListener('click', () => data && menuAgent());
    modelChip.addEventListener('click', () => data && menuModel());
    modeChip.addEventListener('click', () => { if (!data) return; if (form.repo && (!data.branches || data.branches.repo !== form.repo)) post({ type: 'composerBranches', repo: form.repo }); menuMode(); });
    more.addEventListener('click', () => data && menuMore());
    full.addEventListener('click', () => post({ type: 'command', command: 'overseer.newTask' }));
    const grow = () => { task.style.height = 'auto'; task.style.height = Math.min(320, Math.max(66, task.scrollHeight)) + 'px'; };
    task.addEventListener('input', () => { info = ''; if (sent && sent.phase === 'started' && task.value) endSent(); grow(); renderTarget(); renderMentions(); validate(); });
    program.addEventListener('input', validate);
    task.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); go(); } });
    start.addEventListener('click', go);

    // Choices become the defaults only when an agent starts with them (the launcher saves them then).
    function save() { render(); }
    function go() {
      info = '';
      if (toOverseer()) {
        const text = forOverseer(task.value.trim());
        if (!text) return;
        post({ type: 'overseerSend', text });
        task.value = ''; target = remembered; explicit = false; grow(); render();
        return;
      }
      if (!validate() || starting) return;
      starting = true; start.disabled = true;
      const { prompt, options } = tools.take();
      // AC-259: the field clears the instant the task is sent, and says so until the agent starts
      // (the words come back if it cannot start).
      const text = task.value;
      task.value = ''; grow();
      showSent('sending', text);
      post({ type: 'start', form: { ...form, prompt, options: { effort: options.effort, permission_mode: options.permission_mode, images: options.images }, program: program.value.trim(), args: args.value.trim() || '[]' } });
    }
    const sentPlaceholder = () => (sent.phase === 'sending' ? 'Sent ✓ — starting the agent…' : 'Sent ✓ — the agent is starting');
    function showSent(phase, text) {
      clearTimeout(sentTimer);
      sent = { phase, text: text ?? sent?.text ?? '' };
      box.dataset.sent = phase;
      // Back to "Send off a task" a moment after the agent started.
      if (phase === 'started') sentTimer = setTimeout(endSent, 2500);
      render();
    }
    function endSent() {
      clearTimeout(sentTimer);
      if (!sent) return;
      sent = null; delete box.dataset.sent;
      render();
    }
    function showSentNote() {
      const words = sent.text.trim().split(/\s+/).join(' ');
      const short = words.length > 60 ? words.slice(0, 59) + '…' : words;
      note.className = 'composer-note sent';
      note.replaceChildren(sent.phase === 'sending' ? el('span', 'mini-dot') : ui.icon('check', 'sm'),
        el('span', null, sent.phase === 'sending' ? `Sent${short ? ` “${short}”` : ''} — starting the agent…` : `Sent${short ? ` “${short}”` : ''} — the agent is starting.`));
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
        if (d.sendTo) { remembered = d.sendTo === 'agent' ? 'agent' : 'overseer'; if (!explicit && !task.value) target = remembered; }
        form = { repo: form.repo || def.repo || d.repos[0]?.path, routing: d.autoRouting ? (form.routing || def.routing || 'manual') : 'manual', preferredHarness: form.preferredHarness ?? def.preferredHarness ?? '', harness: form.harness || def.harness, account: form.account || def.account, model: form.model ?? def.model ?? '', mode: form.mode || def.mode || 'worktree', approval: form.approval || def.approval, ref: form.ref || '' };
        if (form.repo && !d.repos.some(r => r.path === form.repo)) form.repo = d.repos[0]?.path;
        // No remembered agent: prefer an installed harness with a signed-in account.
        if (!d.harnesses.some(h => h.harness === form.harness && h.installed)) {
          const usable = d.harnesses.filter(h => h.installed && h.harness !== 'generic' && h.harness !== 'codex-app');
          form.harness = (usable.find(h => d.accounts.some(a => a.signedIn && (a.harnesses || []).includes(h.harness))) || usable[0] || {}).harness;
        }
        const compatible = d.accounts.filter(a => (a.harnesses || []).includes(form.harness));
        if (!compatible.some(a => a.id === form.account)) form.account = (compatible.find(a => a.signedIn) || compatible[0] || {}).id;
        if (d.branches) data.branches = d.branches;
        if (form.routing !== 'auto' && window.OverseerContinuity) window.OverseerContinuity.defaults({ data: d, form });
        render(); grow();
      },
      notice(m) {
        // The field was cleared when the task was sent; anything typed since stays (AC-259).
        if (m.kind === 'started') { starting = false; program.value = ''; target = remembered; explicit = false; showSent('started'); onStarted(m.runId); return; }
        if (m.kind === 'repo') { data.repos = [m.repo, ...data.repos.filter(r => r.path !== m.repo.path)]; form.repo = m.repo.path; if (picker) closeRepoPicker({ focus: 'task' }); save(); return; }
        if (m.kind === 'repoError') { if (picker) { picker.busy = ''; picker.error = m.message; renderPicker(); } return; }
        if (m.kind === 'pathHints') { if (picker && picker.input.value.trim() === m.input) { picker.hints = m.hints || []; picker.hintsFor = m.input; renderPicker(); } return; }
        if (m.kind === 'branches') { data.branches = m.branches; menuMode(); return; }
        if (m.kind === 'model') { form.model = m.model; save(); return; }
        // A start that failed puts the words back (unless something new was typed meanwhile).
        const failedStart = starting && sent;
        if (failedStart && !task.value) { task.value = sent.text; grow(); }
        if (failedStart) { clearTimeout(sentTimer); sent = null; delete box.dataset.sent; render(); }
        starting = false; validate();
        if (m.kind === 'info') { info = m.message; showInfo(); return; }
        note.className = 'composer-note error'; note.replaceChildren(ui.icon('error', 'sm'), el('span', null, m.message));
      },
      onState(s) { if (data && s.accounts) { data.accounts = s.accounts; render(); } },
      mentionFiles(m) { tools.files(m); },
      /** Starts an agent from words that went to Overseer by mistake (AC-182's correction). */
      startWith(text) { target = 'agent'; task.value = text; grow(); render(); go(); },
      /** Where Enter sends this time (New Agent: straight to a new agent), not remembered. */
      setTarget(t) { explicit = true; setTarget(t); setTimeout(() => task.focus(), 0); },
      /** Puts words back in the composer for Overseer (after an agent was started by mistake). */
      askOverseer(text) { endSent(); target = 'overseer'; task.value = text; grow(); render(); task.focus(); },
    };
  }
  window.OverseerComposer = { create, fuzzy };
})();
