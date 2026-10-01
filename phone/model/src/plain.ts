// Plain words (AC-228, AC-245): the daemon's and the harnesses' raw texts as the owner reads them.
// Never an internal id, a snake_case state, a tool's internal name, a lowercase harness id, or raw
// git, HTTP or OS error text.
//
// A port of extension/media/plain-words.js, which VS Code's chat, side bar and dialogs use.
// test/plain.test.ts runs the real file on the same texts and compares.

const HARNESS: Readonly<Record<string, string>> = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex', opencode: 'OpenCode', 'opencode-serve': 'Local model', generic: 'Program' };
const TOKENS: Readonly<Record<string, string>> = {
  NOT_FOR_OVERSEER: 'not meant for Overseer', already_answered: 'already answered', waiting_for_user: 'waiting for you', waiting_for_connection: 'waiting for a connection', waiting_for_memory: 'waiting for memory',
  not_sent: 'not sent', not_for_overseer: 'not meant for Overseer', partly_sent: 'partly sent', picked_up: 'picked up', merge_back: 'merge', pull_request: 'pull request', ask_first: 'Ask first', every_turn: 'every turn',
  handed_off: 'handed off', cancel_requested: 'stopping',
};
const ACTION_WORD: Readonly<Record<string, string>> = {
  cadence: 'Changing the check-ins', message: 'Sending the message', start: 'Starting the agent', stop: 'Stopping the agent', permission: 'Answering the permission', merge_back: 'The merge', pull_request: 'The pull request',
  archive: 'Archiving', redirect: 'Redirecting', hold: 'Holding', focus: 'Showing the agent', open_review: 'Opening the review', open_file: 'Opening the file', open_worktree: 'Opening the worktree', show_work: 'Showing the work',
};

const own = (table: Readonly<Record<string, string>>, key: string): string | undefined => (Object.hasOwn(table, key) ? table[key] : undefined);

/** A harness by name: "Claude Code", never "claude". */
export function harnessName(id: unknown): string {
  const s = id ? String(id) : '';
  return own(HARNESS, s) || (s ? s.charAt(0).toUpperCase() + s.slice(1) : '');
}

/** A tool's name as the owner reads it: "get issue (linear)" for `mcp__linear__get_issue`. */
export function plainTool(name: unknown): string {
  const s = String(name || '');
  const m = /^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/.exec(s);
  if (!m) return s;
  const server = m[1] === 'overseer' ? 'Overseer' : String(m[1]).replace(/[_-]+/g, ' ');
  return `${String(m[2]).replace(/_/g, ' ')} (${server})`;
}

/** Raw text in plain words, at most `max` characters. */
export function plain(text: unknown, max = 160): string {
  let t = String(text === null || text === undefined ? '' : text).trim();
  if (!t) return '';
  if (/^NOT_FOR_OVERSEER\W*$/.test(t)) return 'Not meant for Overseer: kept as context.';
  t = t.replace(/^Done:\s*/, '');
  // A usage limit, however the harness says it.
  if (/\[rate_limit\]|\brate[ _-]?limit(?:ed)?\b|\b429\b|usage limit|quota exceeded/i.test(t)) return 'It hit its usage limit. It can go on later, or on another account.';
  t = t.replace(/\bturn reported failure;?\s*(?:last error)?:?\s*/gi, '');
  t = t.replace(/\bAPI Error:?\s*/g, '');
  t = t.replace(/\s*error sending request for url \([^)]*\)/gi, ' could not reach it');
  t = t.replace(/\b([a-z_]+) failed: /g, (_m, a: string) => `${own(ACTION_WORD, a) || 'That'} did not work: `);
  t = t.replace(/\b(?:Error|error|anyhow|panicked at|fatal)[:!]\s*/g, '');
  t = t.replace(/\s*(?:Caused by|caused by|Stack backtrace|stack backtrace)[\s\S]*$/, '');
  t = t.replace(/\s+at \S+:\d+(?::\d+)?/g, '');
  t = t.replace(/\{[^{}]*"[^"]*"\s*:[^{}]*\}/g, '');
  t = t.replace(/\bConnection Failed:?\s*(?:Connect(?: error)?:?\s*)?/gi, '');
  t = t.replace(/\s*\((?:os error \d+|errno \d+)\)/gi, '').replace(/\bConnection refused\b/g, 'could not connect').replace(/\bECONNREFUSED\b/g, 'could not connect').replace(/\bENOENT\b/g, 'not found');
  t = t.replace(/\bhttps?:\/\/(?:127\.0\.0\.1|localhost)[^\s)]*/g, '').replace(/\bHTTP [1-5]\d\d\b/g, '').replace(/\s*\((?:[1-5]\d\d)\)/g, '');
  t = t.replace(/\s*\((?:r|p|sh|w)-[0-9a-f]{6,}\)/g, '').replace(/\b(?:r|p|sh|w)-[0-9a-f]{8,}\b/g, 'it');
  t = t.replace(/\bmcp__[A-Za-z0-9_]+/g, m => plainTool(m));
  t = t.replace(/\bthe (claude|codex|opencode|codex-app) harness\b/g, (_m, h: string) => harnessName(h));
  t = t.replace(/(?<![\w./@~:-])(claude|codex|opencode)(-app|-serve)?(?![\w./@-])/g, (_m, h: string, x: string | undefined) => harnessName(h + (x || '')));
  t = t.replace(/\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b/g, m => own(TOKENS, m) || m.toLowerCase().replace(/_/g, ' '));
  t = t.replace(/\b[a-z]+(?:_[a-z]+)+\b/g, m => own(TOKENS, m) || m.replace(/_/g, ' '));
  t = t.replace(/^[:;,.\s]+/, '').replace(/\s+([,.;:])/g, '$1').replace(/\(\s*\)/g, '').replace(/\s{2,}/g, ' ').trim();
  if (!t) return '';
  // A sentence starts with a capital; a name (ramCeilingPercent, qwen3-coder) keeps its own.
  if (!/^[a-z]+[A-Z0-9:-]/.test(t)) t = t.charAt(0).toUpperCase() + t.slice(1);
  return t.length > max ? t.slice(0, max - 1).trimEnd() + '…' : t;
}
