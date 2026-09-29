// Plain words (AC-228, AC-245): the daemon's and the harnesses' raw texts as the owner reads them,
// on every surface: home's cards, the side bar, toasts, dialogs and tooltips. Never an internal id,
// a snake_case state, a tool's internal name, a lowercase harness id, or raw git, HTTP or OS error
// text. Pure functions, no DOM and no VS Code. (UMD: `require` in Node, `window.OverseerPlain` in a
// webview.)
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.OverseerPlain = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  const HARNESS = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex', opencode: 'OpenCode', 'opencode-serve': 'Local model', generic: 'Program' };
  const TOKENS = { NOT_FOR_OVERSEER: 'not meant for Overseer', already_answered: 'already answered', waiting_for_user: 'waiting for you', waiting_for_connection: 'waiting for a connection', waiting_for_memory: 'waiting for memory', not_sent: 'not sent', not_for_overseer: 'not meant for Overseer', partly_sent: 'partly sent', picked_up: 'picked up', merge_back: 'merge', pull_request: 'pull request', ask_first: 'Ask first', every_turn: 'every turn', handed_off: 'handed off', cancel_requested: 'stopping' };
  const ACTION_WORD = { cadence: 'Changing the check-ins', message: 'Sending the message', start: 'Starting the agent', stop: 'Stopping the agent', permission: 'Answering the permission', merge_back: 'The merge', pull_request: 'The pull request', archive: 'Archiving', redirect: 'Redirecting', hold: 'Holding', focus: 'Showing the agent', open_review: 'Opening the review', open_file: 'Opening the file', open_worktree: 'Opening the worktree', show_work: 'Showing the work' };

  /** A harness by name: "Claude Code", never "claude". */
  const harness = id => HARNESS[id] || (id ? String(id)[0].toUpperCase() + String(id).slice(1) : '');

  /** A tool's name as the owner reads it: "propose (Overseer)" for `mcp__overseer__propose`. */
  function tool(name) {
    const m = /^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/.exec(String(name || ''));
    if (!m) return String(name || '');
    const server = m[1] === 'overseer' ? 'Overseer' : m[1].replace(/[_-]+/g, ' ');
    return `${m[2].replace(/_/g, ' ')} (${server})`;
  }

  /** Raw text in plain words, at most `max` characters. */
  function plain(text, max = 160) {
    let t = String(text == null ? '' : text).trim();
    if (!t) return '';
    if (/^NOT_FOR_OVERSEER\W*$/.test(t)) return 'Not meant for Overseer: kept as context.';
    t = t.replace(/^Done:\s*/, '');
    // A usage limit, however the harness says it.
    if (/\[rate_limit\]|\brate[ _-]?limit(?:ed)?\b|\b429\b|usage limit|quota exceeded/i.test(t)) return 'It hit its usage limit. It can go on later, or on another account.';
    t = t.replace(/\bturn reported failure;?\s*(?:last error)?:?\s*/gi, '');
    t = t.replace(/\bAPI Error:?\s*/g, '');
    t = t.replace(/\b([a-z_]+) failed: /g, (m, a) => `${ACTION_WORD[a] || 'That'} did not work: `);
    t = t.replace(/\b(?:Error|error|anyhow|panicked at|fatal)[:!]\s*/g, '');
    t = t.replace(/\s*(?:Caused by|caused by|Stack backtrace|stack backtrace)[\s\S]*$/, '');
    t = t.replace(/\s+at \S+:\d+(?::\d+)?/g, '');
    t = t.replace(/\{[^{}]*"[^"]*"\s*:[^{}]*\}/g, '');
    t = t.replace(/\bConnection Failed:?\s*(?:Connect(?: error)?:?\s*)?/gi, "");
    t = t.replace(/\s*\((?:os error \d+|errno \d+)\)/gi, '').replace(/\bConnection refused\b/g, 'could not connect').replace(/\bECONNREFUSED\b/g, 'could not connect').replace(/\bENOENT\b/g, 'not found');
    t = t.replace(/\bhttps?:\/\/(?:127\.0\.0\.1|localhost)[^\s)]*/g, '').replace(/\bHTTP [1-5]\d\d\b/g, '').replace(/\s*\((?:[1-5]\d\d)\)/g, '');
    t = t.replace(/\s*\((?:r|p|sh|w)-[0-9a-f]{6,}\)/g, '').replace(/\b(?:r|p|sh|w)-[0-9a-f]{8,}\b/g, 'it');
    t = t.replace(/\bmcp__[A-Za-z0-9_]+/g, m => tool(m));
    t = t.replace(/\bthe (claude|codex|opencode|codex-app) harness\b/g, (m, h) => harness(h));
    t = t.replace(/(?<![\w./@~:-])(claude|codex|opencode)(-app|-serve)?(?![\w./@-])/g, (m, h, x) => harness(h + (x || '')));
    t = t.replace(/\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b/g, m => TOKENS[m] || m.toLowerCase().replace(/_/g, ' '));
    t = t.replace(/\b[a-z]+(?:_[a-z]+)+\b/g, m => TOKENS[m] || m.replace(/_/g, ' '));
    t = t.replace(/^[:;,.\s]+/, '').replace(/\s+([,.;:])/g, '$1').replace(/\(\s*\)/g, '').replace(/\s{2,}/g, ' ').trim();
    if (!t) return '';
    t = t[0].toUpperCase() + t.slice(1);
    return t.length > max ? t.slice(0, max - 1).trimEnd() + '…' : t;
  }

  /** Who answered and where, in words: "You, in VS Code". */
  function answeredBy(by, surface) {
    const who = { owner: 'You', overseer: 'Overseer', agent: 'An agent', ctl: 'You' }[by] || '';
    const where = { vscode: 'in VS Code', voice: 'by voice', phone: 'from the phone', tui: 'in the terminal', ctl: '', settle: '', mcp: '' }[surface];
    return [who, where === undefined ? '' : where].filter(Boolean).join(', ');
  }

  return { plain, harness, tool, answeredBy, HARNESS, TOKENS };
});
