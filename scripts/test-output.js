'use strict';
// Local, private receipts for test output only. Never record argv/environment.
const fs = require('fs');
const path = require('path');
const MAX_CAPTURE = 256 * 1024 * 1024;
const bytes = value => Buffer.isBuffer(value) ? value : Buffer.from(value || '', 'utf8');

function failureLines(value) {
  const lines = String(value).replace(/\x1b\[[0-9;]*m/g, '').split(/\r?\n/);
  const passing = line => /^\s*test .+ \.\.\. (?:ok|ignored)\s*$/.test(line)
    || /^\s*(?:ok\s+\d+|[✓✔])\b/.test(line);
  const chosen = [];
  const add = index => {
    const line = lines[index];
    if (line && line.trim() && !passing(line) && !chosen.includes(index)) chosen.push(index);
  };
  // Test identities come first even after arbitrarily many passing error-named tests.
  lines.forEach((line, index) => {
    if (/^\s*test .+ \.\.\. FAILED\s*$/.test(line)
      || /^\s*(?:FAIL(?:\s|:)|not ok\s+\d+|[✗×✕])/.test(line)) add(index);
  });
  lines.forEach((line, index) => {
    if (/\bpanicked at\b|^\s*(?:AssertionError|assertion (?:failed|`.+` failed)|error(?:\[|:)|npm (?:ERR!|error)|Error:|ERROR(?:\s|:))/.test(line)) {
      // Rust assertion values and source location are adjacent to the panic.
      for (let i = index; i < Math.min(lines.length, index + 5); i++) add(i);
    }
  });
  lines.forEach((line, index) => {
    if (/^\s*(?:failures:|---- .+ (?:stdout|stderr) ----|test result: FAILED)/.test(line)) add(index);
  });
  if (!chosen.length) {
    // Unknown tool format: keep a bounded tail rather than silently print nothing.
    for (let i = Math.max(0, lines.length - 12); i < lines.length; i++) add(i);
  }
  return chosen.slice(0, 12).map(index => lines[index].slice(0, 200));
}

function capture(limit = MAX_CAPTURE) {
  let retained = 0, truncated = false;
  const streams = { stdout: [], stderr: [] };
  return {
    add(stream, value) {
      const chunk = bytes(value), keep = Math.min(chunk.length, limit - retained);
      if (keep) streams[stream].push(Buffer.from(chunk.subarray(0, keep)));
      retained += keep;
      truncated ||= keep < chunk.length;
    },
    result() {
      return { stdout: Buffer.concat(streams.stdout), stderr: Buffer.concat(streams.stderr), captureTruncated: truncated };
    },
  };
}

function privateDirectory(dir) {
  try { fs.mkdirSync(dir, { mode: 0o700 }); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
  const stat = fs.lstatSync(dir);
  if (!stat.isDirectory() || stat.isSymbolicLink() || (stat.mode & 0o777) !== 0o700
    || (process.getuid && stat.uid !== process.getuid())) {
    throw Object.assign(new Error('unsafe evidence directory'), { code: 'EACCES' });
  }
}

function createEvidence(root, run) {
  let runDir, setupError, sequence = 0;
  try {
    if (!/^[a-zA-Z0-9_-]+$/.test(run)) throw Object.assign(new Error('invalid run'), { code: 'EINVAL' });
    const base = path.join(root, '.test-all-logs');
    privateDirectory(base);
    runDir = path.join(base, run);
    // A run is exclusive; do not reuse/overwrite evidence from a prior process.
    fs.mkdirSync(runDir, { mode: 0o700 });
  } catch (error) { setupError = error.code || 'EIO'; }
  return (name, result, limit = MAX_CAPTURE) => {
    const bounded = capture(limit);
    bounded.add('stdout', result.stdout);
    bounded.add('stderr', result.stderr);
    const streams = bounded.result();
    const metadata = {
      stage: name, status: result.status ?? null, signal: result.signal || null,
      spawn_error: result.error ? (result.error.code || 'UNKNOWN') : null,
      capture_complete: !result.captureTruncated && !streams.captureTruncated && !result.error && !result.signal,
      stdout_bytes: streams.stdout.length, stderr_bytes: streams.stderr.length,
    };
    let dir;
    try {
      if (setupError) throw Object.assign(new Error('evidence unavailable'), { code: setupError });
      const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').slice(0, 64) || 'stage';
      dir = path.join(runDir, `${String(++sequence).padStart(3, '0')}-${slug}`);
      fs.mkdirSync(dir, { mode: 0o700 });
      fs.writeFileSync(path.join(dir, 'stdout.log'), streams.stdout, { flag: 'wx', mode: 0o600 });
      fs.writeFileSync(path.join(dir, 'stderr.log'), streams.stderr, { flag: 'wx', mode: 0o600 });
      fs.writeFileSync(path.join(dir, 'result.json'), JSON.stringify(metadata, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
      return { saved: true, path: dir, ...metadata };
    } catch (error) {
      // Keep running stages so the existing leftovers/lock cleanup still happens.
      return { saved: false, path: dir || runDir || null, evidence_error: error.code || 'EIO', ...metadata };
    }
  };
}

function diagnostics(result, receipt) {
  const lines = [];
  if (result.error) lines.push(`spawn/capture error: ${result.error.code || 'UNKNOWN'}`);
  if (result.signal) lines.push(`terminated by ${result.signal}`);
  if (!receipt.capture_complete) lines.push('capture incomplete; logs contain bounded partial output');
  if (!receipt.saved) lines.push(`raw evidence unavailable: ${receipt.evidence_error}`);
  lines.push(...failureLines(String(result.stdout || '') + String(result.stderr || '')));
  if (receipt.path) lines.push(`raw logs: ${receipt.path}`);
  return lines.map(line => `    ${line}`).join('\n');
}
module.exports = { createEvidence, failureLines, capture, diagnostics };
