#!/usr/bin/env node
// SYNTHETIC account CLI fixture (not a live harness): stands in for `codex` and `claude`
// account commands so account governance can be tested without real logins.
//   codex:  --version | login [--device-auth] | login status | logout      (CODEX_HOME/auth.json)
//   claude: --version | auth login | auth status | auth logout            (CLAUDE_CONFIG_DIR/.fixture-login.json)
// `login` signs in as the account named in $FIXTURE_LOGIN_ACCOUNT_FILE ("name:plan").
//   claude -p (stream-json): one turn that fails with authentication_failed unless signed in,
//           so expired/missing logins and re-sign-in can be exercised end to end.
//   codex exec [resume <id>] --json ... -- <prompt>: one synthetic turn (fails unless signed in);
//           also writes a session log with rate limits ($FIXTURE_CODEX_USED percent of 5 hours).
// Default homes: $OVERSEER_TEST_SYSTEM_HOME or $HOME. Tokens are fake unsigned JWTs.
const fs = require('fs');
const path = require('path');
const args = process.argv.slice(2);
if (args[0] === '--version') { console.log('account-fixture 0.0.0 (synthetic)'); process.exit(0); }
const base = process.env.OVERSEER_TEST_SYSTEM_HOME || process.env.HOME;
const next = () => { const [name, plan] = fs.readFileSync(process.env.FIXTURE_LOGIN_ACCOUNT_FILE, 'utf8').trim().split(':'); return { name, plan: plan || 'plus' }; };
const b64 = o => Buffer.from(JSON.stringify(o)).toString('base64url');
if (args.includes('-p')) {
  const dir = process.env.CLAUDE_CONFIG_DIR || path.join(base, '.claude');
  const out = o => process.stdout.write(JSON.stringify(o) + '\n');
  const sid = 'account-fixture-session';
  require('readline').createInterface({ input: process.stdin }).once('line', () => {
    out({ type: 'system', subtype: 'init', session_id: sid, model: 'fixture', cwd: process.cwd(), tools: [] });
    const file = path.join(dir, '.fixture-login.json');
    if (!fs.existsSync(file)) {
      const msg = 'Failed to authenticate: OAuth session expired and could not be refreshed';
      out({ type: 'assistant', session_id: sid, error: 'authentication_failed', message: { role: 'assistant', content: [{ type: 'text', text: msg }] } });
      out({ type: 'result', subtype: 'error_during_execution', is_error: true, result: msg, session_id: sid, usage: {} });
    } else {
      const who = JSON.parse(fs.readFileSync(file, 'utf8')).email;
      out({ type: 'assistant', session_id: sid, parent_tool_use_id: null, message: { role: 'assistant', content: [{ type: 'text', text: `hello from ${who}` }] } });
      out({ type: 'result', subtype: 'success', is_error: false, result: 'ok', session_id: sid, usage: { input_tokens: 1, output_tokens: 1 } });
    }
    setTimeout(() => process.exit(0), 100);
  });
  return;
}
if (args[0] === 'auth') {
  const dir = process.env.CLAUDE_CONFIG_DIR || path.join(base, '.claude');
  const file = path.join(dir, '.fixture-login.json');
  if (args[1] === 'status') {
    const d = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : null;
    console.log(JSON.stringify(d ? { loggedIn: true, authMethod: 'claude.ai', email: d.email, subscriptionType: d.plan } : { loggedIn: false, authMethod: 'none' }));
  } else if (args[1] === 'login') {
    const a = next(); fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(file, JSON.stringify({ email: `${a.name}@example.invalid`, plan: a.plan }));
    console.log(`Login successful (fixture account ${a.name}).`);
  } else if (args[1] === 'logout') { fs.rmSync(file, { force: true }); console.log('Successfully logged out.'); }
  process.exit(0);
}
const dir = process.env.CODEX_HOME || path.join(base, '.codex');
const auth = path.join(dir, 'auth.json');
if (args[0] === 'exec') {
  const out = o => process.stdout.write(JSON.stringify(o) + '\n');
  const resume = args[1] === 'resume' ? args[2] : undefined;
  const thread = resume || 'fixture-thread-' + process.pid;
  const prompt = args[args.indexOf('--') + 1] || '';
  out({ type: 'thread.started', thread_id: thread });
  out({ type: 'turn.started' });
  if (!fs.existsSync(auth)) { out({ type: 'error', message: 'unexpected status 401 Unauthorized: token expired' }); out({ type: 'turn.failed', error: { message: 'unexpected status 401 Unauthorized' } }); process.exit(1); }
  const used = Number(process.env.FIXTURE_CODEX_USED || 20);
  const day = path.join(dir, 'sessions', '2026', '09', '26'); fs.mkdirSync(day, { recursive: true });
  const reset = Math.floor(Date.now() / 1000) + 3600;
  fs.appendFileSync(path.join(day, `rollout-fixture-${thread}.jsonl`), JSON.stringify({ type: 'event_msg', payload: { type: 'token_count', rate_limits: { primary: { used_percent: used, window_minutes: 300, resets_at: reset }, secondary: { used_percent: 40, window_minutes: 10080, resets_at: reset + 86400 * 3 }, plan_type: 'plus' } } }) + '\n');
  setTimeout(() => {
    out({ type: 'item.completed', item: { id: 'item_0', type: 'agent_message', text: `codex fixture reply to: ${prompt.split('\n')[0]}` } });
    out({ type: 'turn.completed', usage: { input_tokens: 12, cached_input_tokens: 0, output_tokens: 5 } });
    process.exit(0);
  }, Number(process.env.FIXTURE_CODEX_DELAY_MS || 200));
  return;
}
if (args[0] === 'login' && args[1] === 'status') {
  if (fs.existsSync(auth)) { console.log('Logged in using ChatGPT'); process.exit(0); }
  console.log('Not logged in'); process.exit(1);
} else if (args[0] === 'login') {
  const a = next(); fs.mkdirSync(dir, { recursive: true });
  const claims = { sub: 'user-' + a.name, 'https://api.openai.com/auth': { chatgpt_account_id: 'acct-' + a.name, chatgpt_user_id: 'user-' + a.name, chatgpt_plan_type: a.plan } };
  fs.writeFileSync(auth, JSON.stringify({ auth_mode: 'chatgpt', tokens: { id_token: `${b64({ alg: 'none' })}.${b64(claims)}.`, access_token: 'fixture', refresh_token: 'fixture' } }));
  console.log(`Successfully logged in${args.includes('--device-auth') ? ' with a device code' : ''} (fixture account ${a.name}).`);
} else if (args[0] === 'logout') { fs.rmSync(auth, { force: true }); console.log('Successfully logged out'); }
