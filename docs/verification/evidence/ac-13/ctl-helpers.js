// Shared helpers for the AC-11/AC-13 live session. Never prints tokens.
const cp = require('child_process'); const path = require('path'); const os = require('os'); const fs = require('fs');
const BIN = path.join(os.homedir(), '.vscode/extensions/beelol.overseer-0.1.0/bin/overseerd-darwin-arm64');
const HOME = path.join(os.homedir(), 'Library/Application Support/Overseer');
const env = { ...process.env, OVERSEER_HOME: HOME };
const ctl = (m, p = {}) => { const o = JSON.parse(cp.execFileSync(BIN, ['ctl', m, JSON.stringify(p)], { env, encoding: 'utf8' }).split('\n')[0]); if (o.error) throw new Error(o.error.message); return o.result; };
const ident = id => { const s = ctl('profile.status', { id }); return { logged_in: s.logged_in, method: s.method, plan: s.identity?.plan, account: (s.identity?.account_fingerprint || s.identity?.fingerprint || '').slice(0, 8), api_key: s.identity?.has_api_key || false }; };
module.exports = { ctl, ident, HOME, fs, path };
