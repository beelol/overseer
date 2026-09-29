// Phone access on the Mac: the pure parts (extension/src/phone-text.js). The status bar's words,
// relative times, the Devices rows, the owner's question, and the QR code: what the vendored
// encoder draws is read back by an independent reader (test/ui/qr-decode.js) and must be the
// pairing code, character for character.
// Run: node test/unit/phone-access.js
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const t = require('../../extension/src/phone-text');
const { decode } = require('../ui/qr-decode');

let failures = 0, passed = 0;
const test = (name, fn) => { try { fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };

const NOW = Date.UTC(2026, 8, 26, 12, 0, 0);
const device = (over = {}) => ({ id: 'd1', name: "Bilal's iPhone", platform: 'ios', scope: 'full', fingerprint: '0123456789abcdef', paired_ms: NOW - 86400000, last_seen_ms: NOW - 5 * 60000, address: '192.168.1.23', revoked_ms: null, connected: false, app: '0.1.0', ...over });
const status = (over = {}) => ({ enabled: true, port: 47810, sessions: 0, paired: 0, devices: [], pairing: null, settings: { notifications: true, allow: [], port: 47810 }, awake: false, ...over });

test('the status bar says off, on, and how many phones', () => {
  assert.strictEqual(t.statusBar(status({ enabled: false })).text, '$(device-mobile) Phone access off');
  assert.strictEqual(t.statusBar(status()).text, '$(device-mobile) Phone access on');
  assert.strictEqual(t.statusBar(status({ devices: [device({ connected: true })] })).text, '$(device-mobile) 1 phone');
  assert.strictEqual(t.statusBar(status({ devices: [device({ connected: true }), device({ id: 'd2', name: 'Pixel', connected: true })] })).text, '$(device-mobile) 2 phones');
});
test('one phone with two sessions is one phone; a revoked or absent phone is not counted', () => {
  const s = status({ sessions: 2, devices: [device({ connected: true }), device({ id: 'd2', connected: false }), device({ id: 'd3', connected: true, revoked_ms: NOW })] });
  assert.strictEqual(t.statusBar(s).phones, 1);
  assert.strictEqual(t.statusBar(s).text, '$(device-mobile) 1 phone');
});
test('with no daemon, or a daemon without phone access, the item is hidden', () => {
  assert.strictEqual(t.statusBar(status(), { online: false }).show, false);
  assert.strictEqual(t.statusBar(undefined, { online: true }).show, false);
  assert.strictEqual(t.statusBar(status(), { available: false }).show, false);
  assert.strictEqual(t.statusBar(status()).show, true);
});
test('the tooltip names the phones, the paired count and the notifications switch', () => {
  const tip = t.statusBar(status({ devices: [device({ connected: true }), device({ id: 'd2', name: 'Pixel' })], settings: { notifications: false }, awake: true })).tooltip;
  assert.match(tip, /1 phone connected: Bilal's iPhone/);
  assert.match(tip, /2 phones paired\./);
  assert.match(tip, /Notifications to phones are off\./);
  assert.match(tip, /Keeping this Mac awake/);
  assert.match(t.statusBar(status({ enabled: false })).tooltip, /Phone access is off\. No phone can connect\./);
  assert.doesNotMatch(t.statusBar(status()).tooltip, /gateway/i);
});
test('the accessible name says the state in words', () => {
  assert.strictEqual(t.statusBar(status({ enabled: false })).label, 'Phone access off');
  assert.strictEqual(t.statusBar(status()).label, 'Phone access on, no phone connected');
  assert.strictEqual(t.statusBar(status({ devices: [device({ connected: true })] })).label, 'Phone access on, 1 phone connected');
});
test('relative times', () => {
  assert.strictEqual(t.ago(NOW - 10000, NOW), 'just now');
  assert.strictEqual(t.ago(NOW - 59999, NOW), 'just now');
  assert.strictEqual(t.ago(NOW - 60000, NOW), '1m ago');
  assert.strictEqual(t.ago(NOW - 59 * 60000, NOW), '59m ago');
  assert.strictEqual(t.ago(NOW - 3600000, NOW), '1h ago');
  assert.strictEqual(t.ago(NOW - 47 * 3600000, NOW), '1d ago');
  assert.strictEqual(t.ago(NOW - 6 * 86400000, NOW), '6d ago');
  assert.match(t.ago(NOW - 30 * 86400000, NOW), /^on /);
  assert.strictEqual(t.ago(NOW + 5000, NOW), 'just now', 'a clock a little ahead is not the future');
  assert.strictEqual(t.ago(0, NOW), '');
});
test('a device row: connected or last seen, and its scope', () => {
  assert.strictEqual(t.deviceRow(device(), NOW).description, 'last seen 5m ago · Full control');
  assert.strictEqual(t.deviceRow(device({ connected: true, scope: 'watch' }), NOW).description, 'connected · Watch only');
  assert.strictEqual(t.deviceRow(device({ last_seen_ms: null }), NOW).description, 'not seen yet · Full control');
  assert.strictEqual(t.deviceRow(device({ connected: true }), NOW).context, 'device-full-connected');
  assert.strictEqual(t.deviceRow(device({ scope: 'watch' }), NOW).context, 'device-watch');
  assert.strictEqual(t.deviceRow(device({ platform: 'android' }), NOW).aria, "Bilal's iPhone, Android phone, last seen 5m ago, Full control");
  const details = Object.fromEntries(t.deviceRow(device(), NOW).details);
  assert.deepStrictEqual(Object.keys(details), ['Phone', 'Scope', 'Paired', 'Last seen', 'Address', 'Key']);
  assert.strictEqual(details.Scope, 'Full control');
  assert.strictEqual(details.Phone, 'iPhone · Overseer 0.1.0');
  assert.strictEqual(details.Address, '192.168.1.23');
  assert.strictEqual(details.Key, '0123 4567 89ab cdef');
});
test('the countdown', () => {
  assert.strictEqual(t.countdown(120000), '2:00');
  assert.strictEqual(t.countdown(119001), '2:00');
  assert.strictEqual(t.countdown(61000), '1:01');
  assert.strictEqual(t.countdown(7000), '0:07');
  assert.strictEqual(t.countdown(-5), '0:00');
});
test("the owner's question names the phone, its address and its key", () => {
  const q = t.pairQuestion({ name: "Bilal's iPhone", platform: 'ios', address: '192.168.1.23', fingerprint: '0123456789abcdef' });
  assert.strictEqual(q.message, 'Pair "Bilal\'s iPhone"?');
  assert.match(q.detail, /^iPhone · 192\.168\.1\.23\nKey 0123 4567 89ab cdef\n\n/);
  assert.deepStrictEqual([q.accept, q.decline], ['Pair', "Don't Pair"]);
});
test('why pairing ended, in plain words', () => {
  assert.strictEqual(t.pairingEnded('declined or not confirmed', 'Pixel'), '"Pixel" was not paired.');
  assert.strictEqual(t.pairingEnded('too many failed attempts'), 'Pairing stopped after too many wrong codes.');
  assert.strictEqual(t.pairingEnded('cancelled on the Mac'), 'Pairing was cancelled.');
  assert.strictEqual(t.pairingEnded('something new'), 'This code no longer works.');
});
test('the code in groups is the code, character for character', () => {
  const code = 'OVSR1-AHKKWZKCLJCHKSH6KKYAPUOMUUQGDALLBECJJ63CIT3YOHOYNDESYESQ6RKRXYF5PW';
  const groups = t.codeGroups(code);
  assert.strictEqual(groups.join(''), code);
  assert.strictEqual(groups[0], 'OVSR1-');
  assert.ok(groups.slice(1, -1).every(g => g.length === 4));
});

// A pairing code as the daemon writes it (docs/rfcs/phone-remote-protocol.md): 134 characters with two addresses.
const CODE = 'OVSR1-AHKKWZKCLJCHKSH6KKYAPUOMUUQGDALLBECJJ63CIT3YOHOYNDESYESQ6RKRXYF5PWRJGVKJP3WEJF7EHABA4MJZGIXDCNRYFY2TALRRHA2ASMJSG4XDALRQFYYQ';
test('the QR code of a pairing code reads back as exactly that code', () => {
  const m = t.qrMatrix(CODE);
  assert.strictEqual(m.rows.length, m.size);
  assert.ok(m.rows.every(r => r.length === m.size && r.every(v => v === 0 || v === 1)));
  const read = decode(m.rows);
  assert.strictEqual(read.text, CODE);
  assert.strictEqual(read.level, 'M');
  assert.strictEqual(m.size, read.version * 4 + 17);
});
test('codes of every length from 1 to 400 characters read back, at each error-correction level', () => {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let versions = new Set();
  for (const level of ['L', 'M', 'Q', 'H']) {
    for (let n = 1; n <= 400; n += level === 'M' ? 1 : 13) {
      let text = 'OVSR1-'.slice(0, n);
      for (let i = 0; text.length < n; i++) text += A[(i * 11 + n) % 32];
      const read = decode(t.qrMatrix(text, level).rows);
      assert.strictEqual(read.text, text, `${level} ${n}`);
      assert.strictEqual(read.level, level);
      versions.add(read.version);
    }
  }
  assert.ok(versions.size >= 15, `covers many sizes: ${[...versions].join(',')}`);
});
test('names with accents and emoji read back', () => {
  const text = 'Zoë’s phone 📱';
  assert.strictEqual(decode(t.qrMatrix(text).rows).text, text);
});
test('one wrong module is noticed by the reader', () => {
  const m = t.qrMatrix(CODE);
  m.rows[20][20] ^= 1;
  assert.throws(() => decode(m.rows), /check|damaged/);
});
test('the SVG path draws exactly the dark modules, inside a quiet zone of four', () => {
  const m = t.qrMatrix(CODE);
  const p = t.qrPath(m);
  assert.strictEqual(p.side, m.size + 8);
  assert.strictEqual(p.quiet, 4);
  assert.match(p.d, /^(M\d+ \d+h\d+v1h-\d+z)+$/);
  const back = t.qrFromPath(p.d, p.side, p.quiet);
  assert.deepStrictEqual(back.rows, m.rows);
  assert.strictEqual(decode(back.rows).text, CODE);
  const xs = [...p.d.matchAll(/M(\d+) (\d+)h(\d+)/g)].map(x => [Number(x[1]), Number(x[2]), Number(x[3])]);
  assert.ok(xs.every(([x, y, w]) => x >= 4 && y >= 4 && x + w <= p.side - 4 && y < p.side - 4), 'nothing is drawn in the quiet zone');
});
test('the vendored encoder is the recorded file', () => {
  const file = fs.readFileSync(path.join(__dirname, '../../extension/media/vendor/toqr.js'));
  const hash = require('crypto').createHash('sha256').update(file).digest('hex');
  const notice = fs.readFileSync(path.join(__dirname, '../../extension/NOTICE.md'), 'utf8');
  assert.ok(notice.includes(hash), 'NOTICE.md records the SHA-256 of media/vendor/toqr.js');
  assert.ok(fs.existsSync(path.join(__dirname, '../../extension/media/vendor/licenses/toqr-LICENSE.md')));
  assert.doesNotMatch(file.toString('utf8'), /https?:\/\/|fetch\(|XMLHttpRequest|require\(/, 'the encoder reaches for nothing');
});
test('nothing the owner reads says "gateway"', () => {
  const src = ['extension/src/phone-text.js', 'extension/src/phone-access.js', 'extension/media/pairing.js'].map(f => fs.readFileSync(path.join(__dirname, '../..', f), 'utf8'));
  // Strings and template text only: method names (gateway.status) and event kinds are the daemon's protocol, not words on screen.
  for (const text of src) for (const m of text.matchAll(/(['"`])((?:\\.|(?!\1).)*)\1/g)) {
    const s = m[2];
    if (/^gateway[._]|^gateway_|^pairing_|^device_/.test(s) || /^[a-z_.]+$/.test(s)) continue;
    assert.doesNotMatch(s, /gateway/i, s);
  }
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, '../../extension/package.json'), 'utf8'));
  const shown = [...manifest.contributes.commands.map(c => c.title), ...manifest.contributes.viewsWelcome.map(w => w.contents), ...Object.values(manifest.contributes.views).flat().map(v => v.name)];
  for (const s of shown) assert.doesNotMatch(s, /gateway/i, s);
});
test('the manifest has the phone access commands, trusted-only where they change something', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, '../../extension/package.json'), 'utf8'));
  const by = Object.fromEntries(manifest.contributes.commands.map(c => [c.title, c]));
  for (const title of ['Turn On Phone Access', 'Turn Off Phone Access', 'Pair a Phone…', 'Revoke Device…', 'Make Watch Only', 'Give Full Control', 'Rename…', 'Turn Off Notifications to Phones', 'Turn On Notifications to Phones']) {
    assert.ok(by[title], title);
    assert.strictEqual(by[title].category, 'Overseer');
    assert.strictEqual(by[title].enablement, 'isWorkspaceTrusted', title);
    assert.ok(by[title].icon, `${title} has an icon`);
  }
  assert.ok(by['Show Devices'] && !by['Show Devices'].enablement);
  const views = manifest.contributes.views.overseer.map(v => v.name);
  assert.deepStrictEqual(views, ['Search', 'Agents', 'Worktree', 'Accounts', 'Devices']); // Worktree: the agent's head (AC-233)
  const inline = manifest.contributes.menus['view/item/context'].filter(m => /overseer\.devices/.test(m.when) && /^inline/.test(m.group)).map(m => m.command);
  assert.deepStrictEqual([...new Set(inline)].sort(), ['overseer.giveDeviceFullControl', 'overseer.makeDeviceWatchOnly', 'overseer.renameDevice', 'overseer.revokeDevice']);
});

console.log(failures ? `${failures} of ${passed + failures} failed` : `${passed} passed`);
process.exit(failures ? 1 : 0);
