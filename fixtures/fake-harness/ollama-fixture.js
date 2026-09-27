#!/usr/bin/env node
// SYNTHETIC `ollama` for the install tests (AC-90) and the packaged-UI scenarios (Gate L): not
// Ollama, and no model ever runs.
//
//   ollama --version        prints a version
//   ollama serve            listens on $OLLAMA_HOST (host:port) and answers /api/version, /api/tags,
//                           /api/show, /api/ps, /api/generate (a load; keep_alive 0 unloads) and
//                           /api/create (a derived tag that shares the weights); ends on SIGTERM
//
// $OLLAMA_FIXTURE_MODELS names a JSON file of installed models ([{tag, size, arch, blocks, kv,
// key, parameters, max_context, num_ctx, capabilities, loaded_size}]); without it nothing is
// installed. Every start is appended to $OLLAMA_FIXTURE_LOG as one JSON line with the address it
// bound; a request log goes to $OLLAMA_FIXTURE_REQUESTS.
'use strict';
const fs = require('fs');
const http = require('http');
const args = process.argv.slice(2);
if (args[0] === '--version' || args[0] === '-v') { console.log('ollama version is 0.0.0-fixture'); process.exit(0); }
if (args[0] !== 'serve') { console.error('the fixture only serves'); process.exit(2); }
const [host, port] = (process.env.OLLAMA_HOST || '127.0.0.1:11434').split(':');
const latest = name => (name.split('/').pop().includes(':') ? name : name + ':latest');
const models = new Map();
const loaded = new Map();
const G = 2 ** 30;
function install(m) {
  const arch = m.arch || 'qwen3moe';
  const details = { parent_model: m.parent || '', format: 'gguf', family: arch, families: [arch], parameter_size: `${((m.parameters || 0) / 1e9).toFixed(1)}B`, quantization_level: 'Q4_K_M', context_length: m.max_context || 32768 };
  const info = { 'general.architecture': arch, 'general.parameter_count': m.parameters || 0 };
  info[`${arch}.block_count`] = m.blocks || 48; info[`${arch}.context_length`] = m.max_context || 32768;
  info[`${arch}.attention.head_count_kv`] = m.kv ?? 4; info[`${arch}.attention.key_length`] = m.key || 128; info[`${arch}.attention.value_length`] = m.key || 128;
  models.set(m.tag, { entry: { name: m.tag, model: m.tag, size: m.size, digest: String(m.size).padStart(64, '0'), details, capabilities: m.capabilities || ['completion', 'tools'] },
    show: { parameters: m.num_ctx ? `num_ctx                        ${m.num_ctx}` : 'temperature                    0.7', details, capabilities: m.capabilities || ['completion', 'tools'], model_info: info }, loaded_size: m.loaded_size });
}
try { for (const m of JSON.parse(fs.readFileSync(process.env.OLLAMA_FIXTURE_MODELS, 'utf8'))) install(m); } catch { /* nothing installed */ }
const log = (o) => { if (process.env.OLLAMA_FIXTURE_REQUESTS) fs.appendFileSync(process.env.OLLAMA_FIXTURE_REQUESTS, JSON.stringify(o) + '\n'); };

const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', d => { body += d; });
  req.on('end', () => {
    let b = {}; try { b = JSON.parse(body || '{}'); } catch { /* not JSON */ }
    log({ method: req.method, path: req.url, body: b });
    const answer = (code, v) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(v)); };
    if (req.url === '/api/version') return answer(200, { version: '0.0.0-fixture' });
    if (req.url === '/api/tags') return answer(200, { models: [...models.values()].map(m => m.entry) });
    if (req.url === '/api/ps') return answer(200, { models: [...loaded.values()] });
    if (req.url === '/api/show') { const m = models.get(latest(b.model || '')) || models.get(b.model); return m ? answer(200, m.show) : answer(404, { error: 'model not found' }); }
    if (req.url === '/api/create') {
      const from = models.get(b.from); if (!from) return answer(404, { error: 'model not found' });
      const name = latest(b.model);
      const copy = JSON.parse(JSON.stringify(from));
      copy.entry.name = name; copy.entry.model = name; copy.entry.details.parent_model = b.from; copy.show.details.parent_model = b.from;
      if (b.parameters && b.parameters.num_ctx) copy.show.parameters = `num_ctx                        ${b.parameters.num_ctx}`;
      models.set(name, copy);
      return answer(200, { status: 'success' });
    }
    if (req.url === '/api/generate') {
      const tag = latest(b.model || '');
      if (b.keep_alive === 0) { loaded.delete(tag); return answer(200, { model: tag, done: true, done_reason: 'unload' }); }
      const m = models.get(tag); if (!m) return answer(404, { error: `model '${tag}' not found` });
      const size = m.loaded_size || m.entry.size + 6 * G;
      loaded.set(tag, { name: tag, model: tag, size, size_vram: size, context_length: (b.options && b.options.num_ctx) || 4096, expires_at: '2026-09-27T00:00:00Z' });
      return setTimeout(() => answer(200, { model: tag, done: true, done_reason: 'load' }), Number(process.env.OLLAMA_FIXTURE_LOAD_MS || 300));
    }
    answer(404, { error: 'the fixture does not answer this' });
  });
});
server.listen(Number(port), host, () => {
  if (process.env.OLLAMA_FIXTURE_LOG) fs.appendFileSync(process.env.OLLAMA_FIXTURE_LOG, JSON.stringify({ pid: process.pid, bound: server.address(), asked: process.env.OLLAMA_HOST }) + '\n');
});
process.on('SIGTERM', () => server.close(() => process.exit(0)));
process.on('SIGINT', () => server.close(() => process.exit(0)));
