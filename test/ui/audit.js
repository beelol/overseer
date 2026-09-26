// Presentation audit (AC-54) run inside a page or webview: visible text, overflow, long unbroken
// text runs and icon-only controls without a name or tooltip. Returns plain JSON.
//
//   root     CSS selector of the view's root (default: body)
//   exclude  CSS selectors whose subtrees are not part of this view
//
// Text inside Monaco diff editors (code) is not counted. "Visible" follows checkVisibility(): text in closed <details>, [hidden] or display:none parts is
// not counted; text scrolled out of view is (it is part of the view).
function auditExpression({ root = 'body', exclude = [] } = {}) {
  return `(() => {
  const root = document.querySelector(${JSON.stringify(root)});
  if (!root) return { missing: ${JSON.stringify(root)} };
  const excluded = ${JSON.stringify(exclude)}.flatMap(s => [...document.querySelectorAll(s)]);
  const outside = n => excluded.some(x => x.contains(n));
  const visible = e => e && (e.checkVisibility ? e.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true }) : e.offsetParent !== null);
  const CODE = 'pre, code, textarea, .code, .monaco-editor, .view-lines';
  let chars = 0; const longRuns = []; const words = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const p = n.parentElement;
    if (!p || ['SCRIPT', 'STYLE', 'NOSCRIPT'].includes(p.tagName) || outside(p) || !visible(p)) continue;
    // Code shown in diff editors is the same content before and after; it is not UI text.
    if (p.closest('.monaco-editor')) continue;
    const text = n.textContent.replace(/\\s+/g, ' ').trim();
    if (!text) continue;
    chars += text.length;
    words.push(text);
    if (p.closest(CODE)) continue;
    for (const token of text.split(' ')) if (token.length > 80) longRuns.push(token.slice(0, 120));
  }
  // Placeholders of empty fields are visible text too.
  for (const f of root.querySelectorAll('input, textarea')) {
    if (outside(f) || !visible(f) || f.value) continue;
    const ph = (f.getAttribute('placeholder') || '').trim(); chars += ph.length; if (ph) words.push(ph);
  }
  // Overflow: the page scrolls sideways, or an element pokes out past the right edge without a
  // clipping or scrolling ancestor.
  const W = innerWidth;
  const clipped = e => { for (let a = e.parentElement; a && a !== document.documentElement; a = a.parentElement) { const ox = getComputedStyle(a).overflowX; if (ox !== 'visible') return true; } return false; };
  const overflow = [];
  if (document.documentElement.scrollWidth > W + 1) overflow.push({ page: true, scrollWidth: document.documentElement.scrollWidth, innerWidth: W });
  for (const e of root.querySelectorAll('*')) {
    if (outside(e) || !visible(e)) continue;
    const r = e.getBoundingClientRect();
    if (r.width && r.right > W + 1 && !clipped(e)) overflow.push({ tag: e.tagName.toLowerCase(), cls: String(e.className).slice(0, 60), right: Math.round(r.right), W });
    if (overflow.length > 20) break;
  }
  // Icon-only controls need an accessible name and a tooltip.
  const unnamed = [];
  for (const b of root.querySelectorAll('button, [role=button], a[href], [role=tab], [role=menuitem]')) {
    if (outside(b) || !visible(b)) continue;
    const text = b.textContent.replace(/\\s+/g, '').trim();
    const iconOnly = text.length <= 2;
    if (!iconOnly) continue;
    const name = b.getAttribute('aria-label') || b.getAttribute('aria-labelledby');
    const tip = b.getAttribute('title') || b.closest('[title]')?.getAttribute('title') || b.getAttribute('data-tooltip');
    if (!name || !tip) unnamed.push({ html: b.outerHTML.slice(0, 100), name: !!name, tip: !!tip });
  }
  return { chars, longRuns: longRuns.slice(0, 20), overflow, unnamed, sample: words.join(' | ').slice(0, 400), width: W };
})()`;
}

module.exports = { auditExpression };
