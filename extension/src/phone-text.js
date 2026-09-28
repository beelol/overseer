// Words and shapes for phone access: the status bar, the Devices list, the pairing panel and its
// QR code. No VS Code API here, so test/unit/phone-access.js runs it in plain Node.
const { toQR } = require('../media/vendor/toqr.js');

const PLATFORM = { ios: 'iPhone', android: 'Android phone' };
const SCOPE = { full: 'Full control', watch: 'Watch only' };
// toqr's error-correction levels (the two format bits of a QR code).
const LEVEL = { M: 0, L: 1, H: 2, Q: 3 };

const plural = (n, one, many = one + 's') => `${n} ${n === 1 ? one : many}`;
const platformName = p => PLATFORM[p] || 'Phone';
const scopeName = s => SCOPE[s] || SCOPE.full;

/** Paired devices that still count: a revoked one is gone from every list. */
const paired = status => (status?.devices || []).filter(d => !d.revoked_ms);
/** Phones connected now. One phone with two sessions is one phone. */
const connected = status => paired(status).filter(d => d.connected);

/**
 * The status bar item. `show` is false while there is nothing true to say (no daemon, or a daemon
 * without phone access).
 */
function statusBar(status, { online = true, available = true } = {}) {
  if (!online || !available || !status) return { show: false, text: '', tooltip: '', label: '', on: false, phones: 0 };
  const on = !!status.enabled;
  const here = connected(status);
  const n = here.length;
  const text = `$(device-mobile) ${!on ? 'Phone access off' : n ? plural(n, 'phone') : 'Phone access on'}`;
  const lines = [];
  if (!on) lines.push('Phone access is off. No phone can connect.');
  else {
    lines.push(n ? `${plural(n, 'phone')} connected: ${here.map(d => d.name).join(', ')}` : 'Phone access is on. No phone is connected.');
    const waiting = status.pairing?.waiting?.length || 0;
    if (waiting) lines.push(`${plural(waiting, 'phone')} waiting to pair.`);
    if (status.awake) lines.push('Keeping this Mac awake while agents run.');
  }
  const total = paired(status).length;
  if (total) lines.push(`${plural(total, 'phone')} paired.`);
  if (status.settings && status.settings.notifications === false) lines.push('Notifications to phones are off.');
  lines.push('Click for phone access.');
  const label = !on ? 'Phone access off' : n ? `Phone access on, ${plural(n, 'phone')} connected` : 'Phone access on, no phone connected';
  return { show: true, text, tooltip: lines.join('\n'), label, on, phones: n };
}

/** "5m ago", "2h ago", "3d ago", then a date. */
function ago(ms, now = Date.now()) {
  if (!ms) return '';
  const s = Math.max(0, (now - ms) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  if (s < 86400 * 7) return `${Math.floor(s / 86400)}d ago`;
  return 'on ' + new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

const presence = (device, now = Date.now()) => (device.connected ? 'connected' : device.last_seen_ms ? `last seen ${ago(device.last_seen_ms, now)}` : 'not seen yet');

/** A key fingerprint in groups of four, easier to compare by eye. */
const fingerprint = f => String(f || '').replace(/(.{4})(?=.)/g, '$1 ');

/** One row of the Devices list. */
function deviceRow(device, now = Date.now()) {
  const where = presence(device, now);
  const scope = scopeName(device.scope);
  const when = ms => (ms ? new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : '');
  const details = [
    ['Phone', platformName(device.platform) + (device.app ? ` · Overseer ${device.app}` : '')],
    ['Scope', scope],
    ['Paired', when(device.paired_ms)],
    ['Last seen', device.connected ? 'connected now' : device.last_seen_ms ? when(device.last_seen_ms) : 'not yet'],
    ['Address', device.address || 'not known'],
    ['Key', fingerprint(device.fingerprint)],
  ];
  return {
    label: device.name, description: `${where} · ${scope}`, details,
    aria: `${device.name}, ${platformName(device.platform)}, ${where}, ${scope}`,
    context: `device-${device.scope === 'watch' ? 'watch' : 'full'}${device.connected ? '-connected' : ''}`,
  };
}

/** "1:59", "0:07"; never below 0:00. */
function countdown(ms) {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** The question the owner answers when a phone asks to pair. */
function pairQuestion(request) {
  const detail = [
    [platformName(request.platform), request.address].filter(Boolean).join(' · '),
    request.fingerprint ? `Key ${fingerprint(request.fingerprint)}` : '',
    '',
    'It will be able to watch and control your agents. You can make it watch only, or revoke it, in Devices.',
  ].filter((l, i, all) => l || (i > 0 && all[i - 1])).join('\n');
  return { message: `Pair "${request.name}"?`, detail, accept: 'Pair', decline: "Don't Pair" };
}

/** Why pairing ended, in the owner's words. */
function pairingEnded(reason, name) {
  const r = String(reason || '');
  if (/declined|not confirmed/.test(r)) return name ? `"${name}" was not paired.` : 'The phone was not paired.';
  if (/too many/.test(r)) return 'Pairing stopped after too many wrong codes.';
  if (/cancelled/.test(r)) return 'Pairing was cancelled.';
  if (/expired/.test(r)) return 'This code has expired.';
  if (/off/.test(r)) return 'Phone access was turned off.';
  if (/restart/.test(r)) return 'Overseer restarted, so this code no longer works.';
  return 'This code no longer works.';
}

/**
 * The QR code of `text` as rows of 0 and 1 (1 is a dark module), made on this Mac by the vendored
 * encoder. Level M: a screen has no damage to recover from, and the code stays small.
 */
function qrMatrix(text, level = 'M') {
  const flat = toQR(String(text), LEVEL[level] ?? LEVEL.M);
  const size = Math.round(Math.sqrt(flat.length));
  if (size * size !== flat.length) throw new Error('The QR code is not square.');
  const rows = [];
  for (let y = 0; y < size; y++) rows.push(Array.from(flat.subarray(y * size, (y + 1) * size), v => (v ? 1 : 0)));
  return { size, rows };
}

/**
 * SVG path data for the dark modules, one unit per module, inside a quiet zone of `quiet`
 * modules. Each run of dark modules in a row is one rectangle: "M4 4h7v1h-7z".
 */
function qrPath({ size, rows }, quiet = 4) {
  let d = '';
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      if (!rows[y][x]) continue;
      let w = 1;
      while (x + w < size && rows[y][x + w]) w++;
      d += `M${x + quiet} ${y + quiet}h${w}v1h-${w}z`;
      x += w;
    }
  }
  return { d, side: size + quiet * 2, size, quiet };
}

/** The modules a path from `qrPath` draws, read back (the scenario checks what was rendered). */
function qrFromPath(d, side, quiet = 4) {
  const size = side - quiet * 2;
  const rows = Array.from({ length: size }, () => new Array(size).fill(0));
  for (const m of String(d).matchAll(/M(\d+) (\d+)h(\d+)v1h-(\d+)z/g)) {
    const [x, y, w] = [Number(m[1]) - quiet, Number(m[2]) - quiet, Number(m[3])];
    for (let i = 0; i < w; i++) rows[y][x + i] = 1;
  }
  return { size, rows };
}

/** The code in groups of four for the eye; the characters are unchanged. */
const codeGroups = code => { const s = String(code); const head = s.startsWith('OVSR1-') ? ['OVSR1-'] : []; const rest = s.slice(head.length ? 6 : 0); return [...head, ...(rest.match(/.{1,4}/g) || [])]; };

module.exports = { statusBar, ago, presence, deviceRow, countdown, pairQuestion, pairingEnded, qrMatrix, qrPath, qrFromPath, codeGroups, platformName, scopeName, fingerprint, paired, connected, plural };
