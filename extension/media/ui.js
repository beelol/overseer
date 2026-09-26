// Shared helpers for Overseer webviews: DOM builders, codicons, status icons, the popup menu,
// short paths, relative times and compact numbers. No dependencies; everything uses textContent.
(function () {
  const ui = {};
  ui.home = window.__overseerHome || '';

  ui.el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  };
  ui.icon = (name, cls = '') => { const i = ui.el('span', `codicon codicon-${name} ${cls}`.trim()); i.setAttribute('aria-hidden', 'true'); return i; };

  /** Icon-only button: always has an accessible name and a tooltip. */
  ui.iconButton = (icon, label, { cls = '', action, pressed, shortcut } = {}) => {
    const b = ui.el('button', `icon-btn ${cls}`.trim());
    b.type = 'button'; b.setAttribute('aria-label', label); b.title = shortcut ? `${label} (${shortcut})` : label;
    if (action) b.dataset.action = action;
    if (pressed !== undefined) b.setAttribute('aria-pressed', String(!!pressed));
    b.append(ui.icon(icon));
    return b;
  };

  const STATUS_TEXT = { queued: 'Queued', starting: 'Starting', running: 'Running', waiting_for_user: 'Needs you', completed: 'Done', failed: 'Failed', interrupted: 'Stopped', disconnected: 'Disconnected', unknown: 'Unknown' };
  ui.statusText = s => STATUS_TEXT[s] || String(s || 'unknown').replace(/_/g, ' ');
  /** Status icon: a pulsing dot while active, then check / bell / error / stop. */
  ui.status = (status, attention) => {
    const s = ui.el('span', `status st-${status || 'unknown'}`);
    s.setAttribute('role', 'img'); s.setAttribute('aria-label', ui.statusText(status)); s.title = ui.statusText(status);
    if (['running', 'starting', 'queued'].includes(status)) s.append(ui.el('span', 'dot'));
    else s.append(ui.icon({ waiting_for_user: attention === 'permission' ? 'shield' : 'bell-dot', completed: 'check', failed: 'error', disconnected: 'debug-disconnect', interrupted: 'circle-slash' }[status] || 'question', 'sm'));
    return s;
  };

  /** Logo for a harness or provider, falling back to a codicon. */
  ui.harnessMark = (harness, size = 14) => (window.OverseerLogos && window.OverseerLogos.logo(window.OverseerLogos.forHarness(harness), { size })) || ui.icon(harness === 'generic' ? 'terminal' : 'hubot', 'sm');
  ui.providerMark = (provider, size = 14) => (window.OverseerLogos && window.OverseerLogos.logo(window.OverseerLogos.forProvider(provider), { size })) || ui.icon('account', 'sm');
  ui.HARNESS = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex app-server', opencode: 'OpenCode', generic: 'Program' };

  /** ~/…/last/two for paths; the full path belongs in a tooltip. */
  ui.shortPath = (p, keep = 2) => {
    if (!p) return '';
    let s = String(p);
    if (ui.home && s.startsWith(ui.home + '/')) s = '~' + s.slice(ui.home.length);
    const parts = s.split('/').filter(Boolean);
    if (parts.length <= keep + 1) return s;
    return (s.startsWith('~') ? '~/…/' : '…/') + parts.slice(-keep).join('/');
  };
  ui.basename = p => String(p || '').split('/').filter(Boolean).pop() || String(p || '');
  ui.firstLine = (t, max = 120) => { const l = String(t || '').split('\n').find(x => x.trim()) || ''; return l.length > max ? l.slice(0, max - 1).trimEnd() + '…' : l; };
  ui.compact = n => (typeof n !== 'number' ? '' : n >= 1e6 ? (n / 1e6).toFixed(1).replace(/\.0$/, '') + 'M' : n >= 1e4 ? Math.round(n / 1e3) + 'k' : n >= 1e3 ? (n / 1e3).toFixed(1).replace(/\.0$/, '') + 'k' : String(n));
  ui.ago = ms => {
    if (!ms) return '';
    const s = Math.max(0, (Date.now() - ms) / 1000);
    if (s < 45) return 'now'; if (s < 3600) return Math.round(s / 60) + 'm'; if (s < 86400) return Math.round(s / 3600) + 'h';
    if (s < 86400 * 7) return Math.round(s / 86400) + 'd';
    return new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  };
  ui.duration = ms => { if (!ms && ms !== 0) return ''; const s = Math.round(ms / 1000); return s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`; };

  /** Popup menu anchored to an element. items: {label, icon?, logo?, hint?, checked?, danger?, disabled?, run()} | 'sep' | {head}. */
  let openMenu;
  ui.closeMenu = () => { if (openMenu) { openMenu.el.remove(); openMenu.anchor?.setAttribute('aria-expanded', 'false'); const a = openMenu.anchor; openMenu = undefined; a?.focus?.(); } };
  ui.menu = (anchor, items, { align = 'start', label = 'Menu' } = {}) => {
    const wasSame = openMenu && openMenu.anchor === anchor;
    ui.closeMenu();
    if (wasSame) return;
    const m = ui.el('div', 'menu'); m.setAttribute('role', 'menu'); m.setAttribute('aria-label', label);
    const buttons = [];
    for (const it of items) {
      if (it === 'sep') { m.append(ui.el('div', 'menu-sep')); continue; }
      if (it.head) { m.append(ui.el('div', 'menu-head', it.head)); continue; }
      const b = ui.el('button', 'menu-item' + (it.danger ? ' danger' : ''));
      b.type = 'button'; b.setAttribute('role', it.checked === undefined ? 'menuitem' : 'menuitemradio');
      if (it.checked !== undefined) b.setAttribute('aria-checked', String(!!it.checked));
      if (it.id) b.id = it.id;
      if (it.title) b.title = it.title;
      if (it.disabled) { b.disabled = true; if (it.why) b.title = it.why; }
      b.append(it.logo || ui.icon(it.checked ? 'check' : it.icon || 'blank', 'sm menu-check'));
      b.append(ui.el('span', 'menu-label', it.label));
      if (it.hint) b.append(ui.el('span', 'menu-hint', it.hint));
      b.addEventListener('click', e => { e.stopPropagation(); ui.closeMenu(); it.run && it.run(); });
      m.append(b); buttons.push(b);
    }
    document.body.append(m);
    const r = anchor.getBoundingClientRect(), mr = m.getBoundingClientRect();
    let left = align === 'end' ? r.right - mr.width : r.left;
    left = Math.max(8, Math.min(left, innerWidth - mr.width - 8));
    let top = r.bottom + 4;
    if (top + mr.height > innerHeight - 8) top = Math.max(8, r.top - mr.height - 4);
    m.style.left = left + 'px'; m.style.top = top + 'px';
    anchor.setAttribute('aria-expanded', 'true');
    openMenu = { el: m, anchor };
    const enabled = buttons.filter(b => !b.disabled);
    (enabled.find(b => b.getAttribute('aria-checked') === 'true') || enabled[0])?.focus();
    m.addEventListener('keydown', e => {
      const i = enabled.indexOf(document.activeElement);
      if (e.key === 'ArrowDown') { enabled[(i + 1) % enabled.length]?.focus(); e.preventDefault(); }
      else if (e.key === 'ArrowUp') { enabled[(i - 1 + enabled.length) % enabled.length]?.focus(); e.preventDefault(); }
      else if (e.key === 'Escape') { ui.closeMenu(); e.preventDefault(); e.stopPropagation(); }
      else if (e.key === 'Tab') { ui.closeMenu(); }
    });
    return m;
  };
  document.addEventListener('mousedown', e => { if (openMenu && !openMenu.el.contains(e.target) && !openMenu.anchor.contains(e.target)) ui.closeMenu(); });
  window.addEventListener('blur', () => ui.closeMenu());

  /** Copies text through the host (webviews cannot always write the clipboard). */
  ui.copy = (post, text) => post({ type: 'copy', text: String(text) });

  window.OverseerUI = ui;
})();
