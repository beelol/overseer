// Prompt tools for both composers (AC-60): image attachments (paperclip, paste, drop), @-mentions
// of worktree files (popup fed by the daemon's file search) and per-turn options (model, reasoning
// effort, permission mode) offered only where the harness supports them.
(function () {
  const ui = window.OverseerUI, el = ui.el;
  const OPTIONS = {
    claude: { models: ['sonnet', 'opus', 'haiku'], efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
      modes: [['manual', 'Ask first', 'shield'], ['acceptEdits', 'Accept edits', 'edit'], ['plan', 'Plan only', 'checklist'], ['auto', 'Auto', 'rocket']] },
    codex: { models: ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'], efforts: ['minimal', 'low', 'medium', 'high', 'xhigh'],
      modes: [['workspace-write', 'Can edit', 'edit'], ['read-only', 'Read only', 'eye']] },
    opencode: { models: [], efforts: [], modes: [] },
  };
  const MAX_IMAGES = 4, MAX_BYTES = 5 * 1024 * 1024;

  /**
   * textarea: the prompt field; bar: element that receives the tool buttons; tray: element for
   * attachment and option chips. ctx: { post, harness(), target() -> {workspace_id}|{repo}, onChange() }
   */
  function create(textarea, bar, tray, ctx) {
    const state = { images: [], mentions: new Set(), model: '', effort: '', mode: '' };
    const attach = ui.iconButton('attach', 'Attach images', { cls: 'sm', action: 'attach', shortcut: 'or paste' });
    const tune = ui.iconButton('settings', 'Model, effort and permissions', { cls: 'sm', action: 'tune' }); tune.setAttribute('aria-haspopup', 'menu');
    const file = el('input'); file.type = 'file'; file.accept = 'image/png,image/jpeg,image/gif,image/webp'; file.multiple = true; file.hidden = true;
    bar.append(attach, tune, file);
    const pop = el('div', 'mention-pop'); pop.hidden = true; pop.setAttribute('role', 'listbox'); pop.setAttribute('aria-label', 'Files');
    document.body.append(pop);
    let mention = null, results = [], active = 0, seq = 0;

    const opts = () => OPTIONS[ctx.harness()] || { models: [], efforts: [], modes: [] };
    function renderTray() {
      tray.replaceChildren();
      for (const [i, img] of state.images.entries()) {
        const c = el('span', 'chip attach-chip'); const thumb = el('img'); thumb.src = img.url; thumb.alt = img.name || 'image'; c.append(thumb, el('span', 'chip-label', img.name || 'image'));
        const x = ui.iconButton('close', `Remove ${img.name || 'image'}`, { cls: 'sm' }); x.addEventListener('click', () => { state.images.splice(i, 1); renderTray(); });
        c.append(x); c.title = `${img.name || 'image'} · ${Math.round(img.bytes / 1024)} KB`; tray.append(c);
      }
      const o = opts();
      const set = [state.model, state.effort && `${state.effort} effort`, state.mode && (o.modes.find(m => m[0] === state.mode) || [])[1]].filter(Boolean);
      if (set.length) {
        const c = el('button', 'chip quiet opt-chip'); c.type = 'button'; c.append(ui.icon('settings', 'sm'), el('span', 'chip-label', set.join(' · ')));
        c.title = 'Options for this message (click to change)'; c.addEventListener('click', menu); tray.append(c);
      }
      tray.hidden = !tray.children.length;
      tune.setAttribute('aria-pressed', String(!!set.length));
      tune.disabled = !(o.models.length && !ctx.noModel) && !o.efforts.length && !o.modes.length;
      attach.hidden = !['claude', 'codex'].includes(ctx.harness());
      ctx.onChange && ctx.onChange();
    }
    function menu() {
      const o = opts(); const items = [];
      if (o.models.length && !ctx.noModel) { items.push({ head: 'Model' }, { label: 'Default', icon: 'sparkle', checked: !state.model, run: () => { state.model = ''; renderTray(); } },
        ...o.models.map(m => ({ label: m, icon: 'sparkle', checked: state.model === m, run: () => { state.model = m; renderTray(); } }))); }
      if (o.efforts.length) { if (items.length) items.push('sep'); items.push({ head: 'Reasoning effort' }, { label: 'Default', icon: 'dashboard', checked: !state.effort, run: () => { state.effort = ''; renderTray(); } },
        ...o.efforts.map(e => ({ label: e, icon: 'dashboard', checked: state.effort === e, run: () => { state.effort = e; renderTray(); } }))); }
      if (o.modes.length) { if (items.length) items.push('sep'); items.push({ head: 'Permissions' }, { label: 'Default', icon: 'shield', checked: !state.mode, run: () => { state.mode = ''; renderTray(); } },
        ...o.modes.map(([v, label, icon]) => ({ label, icon, checked: state.mode === v, run: () => { state.mode = v; renderTray(); } }))); }
      // After a choice the user goes on typing (and Enter sends), so focus returns to the prompt.
      ui.menu(tune, items, { label: 'Options', align: 'start', returnFocus: textarea });
    }
    tune.addEventListener('click', menu);
    attach.addEventListener('click', () => file.click());
    file.addEventListener('change', () => { addFiles([...file.files]); file.value = ''; });
    function addFiles(files) {
      for (const f of files) {
        if (!/^image\/(png|jpeg|gif|webp)$/.test(f.type)) { ctx.notice && ctx.notice('Attach PNG, JPEG, GIF or WebP images.'); continue; }
        if (f.size > MAX_BYTES) { ctx.notice && ctx.notice(`${f.name || 'Image'} is larger than 5 MB.`); continue; }
        if (state.images.length >= MAX_IMAGES) { ctx.notice && ctx.notice('Attach at most 4 images per message.'); break; }
        const reader = new FileReader();
        reader.onload = () => { state.images.push({ name: f.name || 'pasted image', mime: f.type, url: reader.result, bytes: f.size }); renderTray(); };
        reader.readAsDataURL(f);
      }
    }
    textarea.addEventListener('paste', e => { const imgs = [...(e.clipboardData?.files || [])].filter(f => f.type.startsWith('image/')); if (imgs.length) { e.preventDefault(); addFiles(imgs); } });
    textarea.addEventListener('dragover', e => { if ([...(e.dataTransfer?.items || [])].some(i => i.type.startsWith('image/'))) e.preventDefault(); });
    textarea.addEventListener('drop', e => { const imgs = [...(e.dataTransfer?.files || [])].filter(f => f.type.startsWith('image/')); if (imgs.length) { e.preventDefault(); addFiles(imgs); } });

    // @-mentions: "@" followed by characters opens a file list for this worktree or repository.
    function currentMention() {
      const pos = textarea.selectionStart, before = textarea.value.slice(0, pos);
      const m = /(^|\s)@([\w./-]*)$/.exec(before);
      return m ? { start: pos - m[2].length - 1, end: pos, query: m[2] } : null;
    }
    function place() {
      const r = textarea.getBoundingClientRect();
      pop.style.left = Math.max(8, r.left) + 'px'; pop.style.width = Math.min(420, r.width) + 'px';
      const h = pop.getBoundingClientRect().height || 200;
      pop.style.top = (r.top - h - 6 > 8 ? r.top - h - 6 : r.bottom + 6) + 'px';
    }
    function renderPop() {
      pop.replaceChildren();
      if (!results.length) { pop.hidden = true; return; }
      results.forEach((f, i) => {
        const row = el('div', 'mention-item'); row.setAttribute('role', 'option'); row.setAttribute('aria-selected', String(i === active));
        row.append(ui.icon('file', 'sm'), el('span', 'mention-name', ui.basename(f)), el('span', 'mention-dir', f.includes('/') ? f.slice(0, f.lastIndexOf('/')) : ''));
        row.title = f; row.addEventListener('mousedown', e => { e.preventDefault(); choose(f); });
        pop.append(row);
      });
      pop.hidden = false; place();
    }
    function choose(f) {
      if (!mention) return;
      const v = textarea.value;
      textarea.value = v.slice(0, mention.start) + '@' + f + ' ' + v.slice(mention.end);
      const caret = mention.start + f.length + 2; textarea.setSelectionRange(caret, caret);
      state.mentions.add(f); mention = null; results = []; renderPop();
      textarea.dispatchEvent(new Event('input'));
    }
    textarea.addEventListener('input', () => {
      mention = currentMention();
      if (!mention) { results = []; renderPop(); return; }
      const target = ctx.target(); if (!target) return;
      const id = ++seq;
      ctx.post({ type: 'mentionFiles', query: mention.query, ...target, seq: id });
    });
    textarea.addEventListener('keydown', e => {
      if (pop.hidden) return;
      if (e.key === 'ArrowDown') { active = (active + 1) % results.length; renderPop(); e.preventDefault(); e.stopImmediatePropagation(); }
      else if (e.key === 'ArrowUp') { active = (active - 1 + results.length) % results.length; renderPop(); e.preventDefault(); e.stopImmediatePropagation(); }
      else if (e.key === 'Enter' || e.key === 'Tab') { choose(results[active]); e.preventDefault(); e.stopImmediatePropagation(); }
      else if (e.key === 'Escape') { results = []; renderPop(); e.preventDefault(); e.stopImmediatePropagation(); }
    }, true);
    textarea.addEventListener('blur', () => setTimeout(() => { results = []; renderPop(); }, 150));

    renderTray();
    return {
      files(m) { if (m.seq !== seq || !mention) return; results = m.files.slice(0, 12); active = 0; renderPop(); },
      /** Prompt text plus the options for daemon requests; clears attachments. */
      take() {
        const text = textarea.value;
        const mentioned = [...state.mentions].filter(f => text.includes('@' + f));
        const prompt = mentioned.length ? `${text.trim()}\n\nFiles mentioned (paths relative to the repository root): ${mentioned.map(f => '`' + f + '`').join(', ')}` : text;
        const options = { model: state.model || undefined, effort: state.effort || undefined, permission_mode: state.mode || undefined,
          images: state.images.length ? state.images.map(i => ({ mime: i.mime, data: String(i.url).split(',')[1], name: i.name })) : undefined };
        state.images = []; state.mentions.clear(); renderTray();
        return { prompt, options };
      },
      reset() { state.images = []; state.mentions.clear(); state.model = ''; state.effort = ''; state.mode = ''; renderTray(); },
      refresh: renderTray,
      state,
    };
  }
  window.OverseerPromptTools = { create, OPTIONS };
})();
