// AC201 evidence receipts for disposable fixture daemons only. No authority or settings changes.
const assert = require('assert');
const cp = require('child_process');
const path = require('path');
function initialSettings(s, result) {
  assert.ok(s.home, 'AC201 receipts must never read the owner daemon');
  const rows = JSON.parse(cp.execFileSync('sqlite3', ['-cmd', '.timeout 5000', '-json', path.join(s.home, 'overseer.sqlite'),
    "SELECT key,value FROM meta WHERE key IN ('overseer.channel','overseer.check_ins') ORDER BY key"], { encoding: 'utf8', timeout: 6000, maxBuffer: 65536 }) || '[]');
  const stored = Object.fromEntries(rows.map(r => [r.key, r.value]));
  const requested = { channel: process.env.OVERSEER_CHANNEL_DEFAULT || 'auto', check_ins: process.env.OVERSEER_CHECK_INS || 'every:3' };
  const actual = { channel: stored['overseer.channel'] || 'auto', check_ins: stored['overseer.check_ins'] || 'every:3' };
  assert.deepStrictEqual(actual, requested, 'AC201 requested environment did not reach this actual fixture daemon');
  result.settings = { requested, actual, effective_global_cadence: s.ctl('agent.cadence', {}).cadence, overrides: [], runs: [] };
  s.note('AC201 initial settings before owner overrides', result.settings);
}
function settingOverride(s, result, control, value, reason) {
  assert.strictEqual(control, 'check_ins', 'Only the actual check-in override is recorded by these scenarios');
  const observed = s.ctl('agent.cadence', {}).cadence;
  assert.strictEqual(observed, value);
  const override = { control, value, observed, reason };
  result.settings.overrides.push(override); s.note('AC201 explicit owner setting override', override);
}
function runSettings(s, result, runId) {
  const run = s.ctl('state').runs.find(r => r.id === runId);
  assert.ok(run, 'AC201 receipt target must be an actual visible fixture agent');
  const channel = s.ctl('agent.channel', { run_id: runId });
  const cadence = s.ctl('agent.cadence', { run_id: runId }).cadence;
  const overrides = result.settings.overrides;
  const expectedCadence = [...overrides].reverse().find(o => o.control === 'check_ins')?.value || result.settings.requested.check_ins;
  assert.strictEqual(cadence, expectedCadence === 'every:1' ? 'every turn' : expectedCadence);
  const expectedChannel = [...overrides].reverse().find(o => o.control === 'channel')?.value || result.settings.requested.channel;
  if (expectedChannel !== 'auto') {
    const enabled = expectedChannel === 'on' && run.harness !== 'generic';
    assert.strictEqual(channel.briefing, enabled); assert.strictEqual(channel.channel, enabled);
  }
  const echoes = s.ctl('events.list', { run_id: runId, limit: 5000 }).events.filter(e => e.kind === 'output' && /^ECHO /.test(e.payload.text || ''));
  const echo = echoes.length ? JSON.parse(echoes[0].payload.text.slice(5)) : null;
  if (echo && ['on', 'off'].includes(expectedChannel) && run.harness === 'claude') assert.strictEqual(echo.argv.includes('--mcp-config'), expectedChannel === 'on', 'actual fixture launch disagrees with channel profile');
  const receipt = { run_id: runId, harness: run.harness, channel, cadence, mcp_configured: echo ? echo.argv.includes('--mcp-config') : null,
    briefings: s.ctl('agent.briefings', { run_id: runId }).briefings.length, after_overrides: overrides.slice() };
  result.settings.runs.push(receipt); s.note('AC201 effective fixture run settings', receipt);
}
module.exports = { initialSettings, settingOverride, runSettings };
