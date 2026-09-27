#!/usr/bin/env node
// SYNTHETIC `ollama` program for the install tests (AC-90): not Ollama, and no model.
//
//   ollama --version        prints a version
//   ollama serve            listens on $OLLAMA_HOST (host:port) and answers /api/version,
//                           /api/tags and /api/ps; ends on SIGTERM
//
// Every start is appended to $OLLAMA_FIXTURE_LOG as one JSON line with the address it bound.
'use strict';
const fs = require('fs');
const http = require('http');
const args = process.argv.slice(2);
if (args[0] === '--version' || args[0] === '-v') { console.log('ollama version is 0.0.0-fixture'); process.exit(0); }
if (args[0] !== 'serve') { console.error('the fixture only serves'); process.exit(2); }
const [host, port] = (process.env.OLLAMA_HOST || '127.0.0.1:11434').split(':');
const server = http.createServer((req, res) => {
  const answer = { '/api/version': { version: '0.0.0-fixture' }, '/api/tags': { models: [] }, '/api/ps': { models: [] } }[req.url];
  res.writeHead(answer ? 200 : 404, { 'content-type': 'application/json' });
  res.end(JSON.stringify(answer || { error: 'the fixture does not answer this' }));
});
server.listen(Number(port), host, () => {
  if (process.env.OLLAMA_FIXTURE_LOG) fs.appendFileSync(process.env.OLLAMA_FIXTURE_LOG, JSON.stringify({ pid: process.pid, bound: server.address(), asked: process.env.OLLAMA_HOST }) + '\n');
});
process.on('SIGTERM', () => server.close(() => process.exit(0)));
