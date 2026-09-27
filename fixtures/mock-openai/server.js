#!/usr/bin/env node
// Deterministic OpenAI-compatible Chat Completions mock (MOCK MODEL RESPONSES, not a real
// model). Used to exercise the real OpenCode harness and Overseer's OpenCode adapter
// without paid tokens. Behaviour is keyed off the latest user text:
//   "CHILD: ..."            -> plain text "hi from child" (runs inside a native task child)
//   contains "delegate"     -> tool call `task` spawning a subagent, then "done"
//   contains "write <file>" -> tool call `write` creating <file> in the working directory, then "done"
//   contains "slow"         -> streams one word per second for 60 seconds (interrupt tests)
//   contains "sequence N"   -> N `edit` tool calls alternating a.txt/b.txt at distant lines
//   contains "sequence3 N"  -> the same across a.txt, b.txt and c.txt
//                              ("L<n>: original" -> "L<n>: agent edit <k>"), paced by MOCK_STEP_DELAY_MS
//   anything else           -> "hello from mock"
// Every request is appended to $MOCK_LOG (JSON lines) for evidence.
const http = require('http');
const fs = require('fs');

const port = Number(process.env.MOCK_PORT || 0);
const logFile = process.env.MOCK_LOG;
let counter = 0;

function textOf(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) return content.map(c => c.text || '').join('');
  return '';
}

function plan(body) {
  const messages = body.messages || [];
  const last = messages[messages.length - 1] || {};
  const system = messages.filter(m => m.role === 'system').map(m => textOf(m.content)).join('\n');
  const cwd = (system.match(/Working directory:\s*(\S+)/) || [])[1] || process.cwd();
  const users = messages.filter(m => m.role === 'user');
  const user = textOf(users[users.length - 1]?.content);
  // "sequence3 N" cycles three files (a.txt, b.txt, c.txt); "sequence N" alternates a.txt/b.txt.
  const three = /sequence3 \d+/i.test(user);
  const sequence = user.match(/sequence3? (\d+)/i);
  if (sequence && (body.tools || []).some(t => t.function?.name === 'edit')) {
    const lastUser = messages.lastIndexOf(users[users.length - 1]);
    const done = messages.slice(lastUser).filter(m => m.role === 'tool').length;
    if (done < Number(sequence[1])) {
      const lines = [20, 280, 60, 240, 150, 200, 100, 30, 260, 180];
      const line = lines[done % lines.length] + Math.floor(done / lines.length);
      const file = `${cwd}/${three ? ['a.txt', 'b.txt', 'c.txt'][done % 3] : done % 2 ? 'b.txt' : 'a.txt'}`;
      return { tool: 'edit', args: { filePath: file, oldString: `L${line}: original`, newString: `L${line}: agent edit ${done + 1}` }, pace: true };
    }
    return { text: `done after ${done} edits` };
  }
  if (last.role === 'tool') return { text: 'done' };
  if (/CHILD: delegate/.test(user) && (body.tools || []).some(t => t.function?.name === 'task')) {
    return { tool: 'task', args: { description: 'grandchild hi', prompt: 'CHILD: reply with hi', subagent_type: 'general' } };
  }
  if (/CHILD:/.test(user)) return { text: 'hi from child' };
  if (/delegate twice/i.test(user) && (body.tools || []).some(t => t.function?.name === 'task')) {
    return { tool: 'task', args: { description: 'child that delegates', prompt: 'CHILD: delegate once more', subagent_type: 'general' } };
  }
  if (/delegate/i.test(user) && (body.tools || []).some(t => t.function?.name === 'task')) {
    return { tool: 'task', args: { description: 'say hi', prompt: 'CHILD: reply with hi', subagent_type: 'general' } };
  }
  const write = user.match(/write\s+([\w./-]+)/i);
  if (write && (body.tools || []).some(t => t.function?.name === 'write')) {
    const file = write[1].startsWith('/') ? write[1] : `${cwd}/${write[1]}`;
    return { tool: 'write', args: { filePath: file, content: `hello from mock turn ${users.length}\n` } };
  }
  if (/slow/i.test(user)) return { text: Array.from({ length: 60 }, (_, i) => `word${i} `), slow: true };
  if (/title|summar/i.test(system) && !body.tools?.length) return { text: 'Mock task' };
  return { text: 'hello from mock' };
}

function chunk(id, delta, finish = null) {
  return `data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: Math.floor(Date.now() / 1000), model: 'mock-coder', choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
}

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url.endsWith('/models')) {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ object: 'list', data: [{ id: 'mock-coder', object: 'model' }] }));
    return;
  }
  let raw = '';
  req.on('data', d => { raw += d; });
  req.on('end', async () => {
    let body = {};
    try { body = JSON.parse(raw || '{}'); } catch {}
    const p = plan(body);
    if (p.pace) await new Promise(r => setTimeout(r, Number(process.env.MOCK_STEP_DELAY_MS || 1500)));
    const id = `mock-${++counter}`;
    if (logFile) fs.appendFileSync(logFile, JSON.stringify({ t: Date.now(), id, url: req.url, stream: !!body.stream, last: (body.messages || []).slice(-1)[0]?.role, plan: p.tool || (p.slow ? 'slow' : 'text') }) + '\n');
    if (!body.stream) {
      res.writeHead(200, { 'content-type': 'application/json' });
      const message = p.tool ? { role: 'assistant', content: null, tool_calls: [{ id: `call_${counter}`, type: 'function', function: { name: p.tool, arguments: JSON.stringify(p.args) } }] }
        : { role: 'assistant', content: Array.isArray(p.text) ? p.text.join('') : p.text };
      res.end(JSON.stringify({ id, object: 'chat.completion', model: 'mock-coder', choices: [{ index: 0, message, finish_reason: p.tool ? 'tool_calls' : 'stop' }], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } }));
      return;
    }
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
    let closed = false;
    req.on('close', () => { closed = true; });
    res.write(chunk(id, { role: 'assistant', content: '' }));
    if (p.tool) {
      res.write(chunk(id, { tool_calls: [{ index: 0, id: `call_${counter}`, type: 'function', function: { name: p.tool, arguments: '' } }] }));
      res.write(chunk(id, { tool_calls: [{ index: 0, function: { arguments: JSON.stringify(p.args) } }] }));
      res.write(chunk(id, {}, 'tool_calls'));
    } else {
      const parts = Array.isArray(p.text) ? p.text : [p.text];
      for (const part of parts) {
        if (closed) return;
        res.write(chunk(id, { content: part }));
        if (p.slow) await new Promise(r => setTimeout(r, 1000));
      }
      res.write(chunk(id, {}, 'stop'));
    }
    res.write(`data: ${JSON.stringify({ id, object: 'chat.completion.chunk', model: 'mock-coder', choices: [], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } })}\n\n`);
    res.write('data: [DONE]\n\n');
    res.end();
  });
});
server.listen(port, '127.0.0.1', () => {
  const actual = server.address().port;
  if (process.env.MOCK_PORT_FILE) fs.writeFileSync(process.env.MOCK_PORT_FILE, String(actual));
  console.log(`mock-openai listening on 127.0.0.1:${actual}`);
});
