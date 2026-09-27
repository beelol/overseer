// Markdown for agent replies (AC-55): marked parses, DOMPurify sanitizes (no raw HTML, no
// scripts, no inline styles), highlight.js colors code. Links never navigate the webview; the
// host opens them. Code blocks get a quiet header with the language and a Copy button.
(function () {
  const ui = window.OverseerUI;
  const md = window.marked;
  if (md && md.setOptions) md.setOptions({ gfm: true, breaks: false });
  const PURIFY = { ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'del', 's', 'code', 'pre', 'blockquote', 'ul', 'ol', 'li', 'a', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'span', 'input'],
    ALLOWED_ATTR: ['href', 'title', 'class', 'type', 'checked', 'disabled', 'align', 'start'], ALLOW_DATA_ATTR: false };

  /** Renders Markdown into `target` (replacing its content). Falls back to plain text. */
  function render(target, text, { post } = {}) {
    const source = String(text || '');
    if (!md || !window.DOMPurify) { target.textContent = source; return; }
    let html;
    try { html = window.DOMPurify.sanitize(md.parse(source), PURIFY); } catch { target.textContent = source; return; }
    target.innerHTML = html;
    for (const a of target.querySelectorAll('a[href]')) {
      const href = a.getAttribute('href');
      a.title = href;
      a.addEventListener('click', e => { e.preventDefault(); if (post) post({ type: 'openExternal', url: href }); });
    }
    for (const input of target.querySelectorAll('input')) input.disabled = true;
    for (const pre of target.querySelectorAll('pre')) decorate(pre, post);
    shortenLong(target, post);
    for (const table of target.querySelectorAll('table')) { const wrap = ui.el('div', 'md-table'); table.replaceWith(wrap); wrap.append(table); }
  }

  /** Long paths and other unbroken tokens in prose become a short label; the full value is the tooltip and a click copies it. */
  function shortenLong(root, post) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const hits = [];
    for (let n = walker.nextNode(); n; n = walker.nextNode()) if (!n.parentElement.closest('pre, a') && /\S{60,}/.test(n.textContent)) hits.push(n);
    for (const n of hits) {
      const frag = document.createDocumentFragment();
      for (const part of n.textContent.split(/(\S{60,})/)) {
        if (!/^\S{60,}$/.test(part)) { frag.append(part); continue; }
        const isPath = part.includes('/');
        const b = ui.el('button', 'md-long', isPath ? ui.shortPath(part.replace(/[.,;:)]+$/, '')) : part.slice(0, 24) + '…' + part.slice(-12));
        b.type = 'button'; b.title = `${part}\nClick to copy`;
        b.addEventListener('click', () => post && post({ type: 'copy', text: part }));
        frag.append(b);
      }
      n.replaceWith(frag);
    }
  }

  function decorate(pre, post) {
    const code = pre.querySelector('code');
    const lang = code && /language-([\w+-]+)/.exec(code.className || '')?.[1];
    if (lang === 'overseer-actions') { proposal(pre, code.textContent, post); return; }
    if (code && window.hljs) {
      try {
        if (lang && window.hljs.getLanguage(lang)) code.innerHTML = window.hljs.highlight(code.textContent, { language: lang, ignoreIllegals: true }).value;
        else if (code.textContent.length < 20000) code.innerHTML = window.hljs.highlightAuto(code.textContent).value;
      } catch { /* plain text is fine */ }
    }
    const block = ui.el('div', 'codeblock');
    const head = ui.el('div', 'codeblock-head');
    head.append(ui.el('span', 'codeblock-lang', lang || 'text'));
    const copy = ui.iconButton('copy', 'Copy code', { cls: 'sm codeblock-copy' });
    copy.addEventListener('click', () => { post && post({ type: 'copy', text: code ? code.textContent : pre.textContent }); copy.replaceChildren(ui.icon('check')); setTimeout(() => copy.replaceChildren(ui.icon('copy')), 1200); });
    head.append(copy);
    pre.replaceWith(block);
    block.append(head, pre);
    const lines = (code ? code.textContent : pre.textContent).split('\n').length;
    if (lines > 24) {
      block.classList.add('collapsed');
      const more = ui.el('button', 'codeblock-more', `Show all ${lines} lines`);
      more.type = 'button';
      more.addEventListener('click', () => { block.classList.remove('collapsed'); more.remove(); });
      block.append(more);
    }
  }

  // Talk to Overseer (AC-107): a proposal from Overseer is a card; nothing happens without a Yes.
  function proposal(pre, text, post) {
    let actions;
    try { actions = JSON.parse(text); if (!Array.isArray(actions)) actions = [actions]; } catch { actions = null; }
    const card = ui.el('div', 'proposal'); card.setAttribute('role', 'group'); card.setAttribute('aria-label', 'Overseer proposes');
    const head = ui.el('div', 'proposal-head'); head.append(ui.icon('eye', 'sm'), ui.el('span', null, 'Overseer will'));
    const list = ui.el('ul', 'proposal-list');
    const say = a => a.action === 'follow_up' ? `Send ${a.title || a.agent}: “${a.text}”` : a.action === 'stop' ? `Stop ${a.title || a.agent}` : a.action === 'pin' ? `Pin ${a.title || a.agent} to the grid` : a.action === 'start' ? `Start “${a.title || a.prompt}” in ${a.repo}` : JSON.stringify(a);
    for (const a of actions || []) list.append(ui.el('li', null, say(a)));
    const status = ui.el('div', 'proposal-status'); status.setAttribute('role', 'status');
    const yes = ui.el('button', 'btn primary sm', 'Yes'); yes.type = 'button'; yes.dataset.proposal = 'yes';
    const no = ui.el('button', 'btn sm', 'No'); no.type = 'button'; no.dataset.proposal = 'no';
    const row = ui.el('div', 'proposal-actions'); row.append(yes, no);
    card.append(head, list, row, status);
    if (!actions) { list.replaceChildren(ui.el('li', null, 'The proposal could not be read, so nothing will be done.')); row.hidden = true; }
    let key = 0; for (let i = 0; i < text.length; i++) key = (Math.imul(key, 31) + text.charCodeAt(i)) | 0;
    key = 'p' + (key >>> 0).toString(16);
    card.dataset.key = key;
    const decide = type => { yes.disabled = no.disabled = true; status.textContent = type === 'overseerActions' ? 'Working…' : 'Declining…'; post && post({ type, actions, key }); };
    yes.addEventListener('click', () => decide('overseerActions'));
    no.addEventListener('click', () => decide('overseerDecline'));
    window.addEventListener('message', e => { if (e.data?.type === 'overseerAnswered' && e.data.key === key) { status.textContent = e.data.text; row.hidden = true; card.classList.add('answered'); } });
    pre.replaceWith(card);
  }

  window.OverseerMarkdown = { render };
})();
