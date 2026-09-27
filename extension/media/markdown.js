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

  window.OverseerMarkdown = { render };
})();
