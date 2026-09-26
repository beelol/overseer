#!/usr/bin/env node
// SYNTHETIC GitHub REST API fixture for Open PR tests (AC-50). Listens on 127.0.0.1:$MOCK_PORT
// (0 = any; the port is written to $MOCK_PORT_FILE), requires `Authorization: Bearer $MOCK_TOKEN`,
// and logs every request (method, path, body; never the token) as JSON lines to $MOCK_LOG.
//   POST /repos/:owner/:repo/pulls        201 {number, html_url} or 422 "already exists"
//   GET  /repos/:owner/:repo/pulls?head=  200 [open pulls for that head]
const http = require('http');
const fs = require('fs');
const token = process.env.MOCK_TOKEN || 'test-token';
const pulls = [];
const log = entry => { if (process.env.MOCK_LOG) fs.appendFileSync(process.env.MOCK_LOG, JSON.stringify(entry) + '\n'); };
const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', c => { body += c; });
  req.on('end', () => {
    const url = new URL(req.url, 'http://127.0.0.1');
    const authorized = req.headers.authorization === `Bearer ${token}`;
    let parsed = null; try { parsed = body ? JSON.parse(body) : null; } catch {}
    log({ method: req.method, path: url.pathname, query: url.search, authorized, body: parsed });
    const send = (status, json) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(json)); };
    if (!authorized) return send(401, { message: 'Bad credentials' });
    const m = url.pathname.match(/^\/repos\/([^/]+)\/([^/]+)\/pulls$/);
    if (!m) return send(404, { message: 'Not Found' });
    const [, owner, repo] = m;
    if (req.method === 'GET') return send(200, pulls.filter(p => p.owner === owner && p.repo === repo && `${owner}:${p.head}` === url.searchParams.get('head')));
    if (req.method !== 'POST' || !parsed?.head || !parsed?.base || !parsed?.title) return send(422, { message: 'Validation Failed' });
    if (pulls.some(p => p.owner === owner && p.repo === repo && p.head === parsed.head)) return send(422, { message: 'Validation Failed', errors: [{ message: `A pull request already exists for ${owner}:${parsed.head}.` }] });
    const number = 41 + pulls.length + 1;
    const port = server.address().port;
    const pr = { owner, repo, head: parsed.head, base: parsed.base, title: parsed.title, body: parsed.body, number, html_url: `http://127.0.0.1:${port}/${owner}/${repo}/pull/${number}` };
    pulls.push(pr);
    send(201, pr);
  });
});
server.listen(Number(process.env.MOCK_PORT || 0), '127.0.0.1', () => { if (process.env.MOCK_PORT_FILE) fs.writeFileSync(process.env.MOCK_PORT_FILE, String(server.address().port)); });
