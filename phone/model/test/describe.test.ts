// What a tool call reads as: the phone's `describe` against the one in VS Code's conversation.js,
// for every tool it knows and for input that is cut off or is not what it should be.
import { describe as group, expect, it } from 'vitest';
import { describe, hostOf, lineCount, parseInput } from '../src/describe.ts';
import { page } from './helpers/vscode.ts';

const p = page();
const theirs = (p.window as unknown as { OverseerConversation: { describe(name: unknown, input: unknown, summary?: unknown): Record<string, unknown> } }).OverseerConversation.describe;

const NAMES = ['Read', 'Write', 'Edit', 'MultiEdit', 'NotebookEdit', 'Grep', 'Glob', 'LS', 'Bash', 'shell', 'command', 'commandExecution', 'apply_patch', 'fileChange', 'WebFetch', 'WebSearch', 'web_search', 'webSearch', 'TodoWrite', 'Agent', 'Task', 'task',
  'collab:spawn_agent', 'spawn_agent', 'collab:wait', 'collab:send_input', 'mcp__linear__get_issue', 'MCPTool', 'Skill', 'SomethingNew', '', undefined, null, 42];
const INPUTS: unknown[] = [
  undefined, null, '', {}, [], 'plain words', 42, { file_path: '/repo/src/a.ts' }, { notebook_path: '/repo/n.ipynb', new_source: 'x\ny' }, { path: '/repo/dir/' }, { file_path: '/repo/a.md', content: 'one\ntwo\nthree\n' }, { file_path: '/repo/a.md', content: '' },
  { file_path: 'a.ts', old_string: 'a\nb\n', new_string: 'c' }, { file_path: 'a.ts', edits: [{ old_string: 'a', new_string: 'b\nc' }, { old_string: 'd\ne', new_string: '' }, {}] }, { edits: 'not a list' },
  { command: 'npm test -- --grep "x"', description: 'Run the tests' }, { command: '\n\n  first line after empty ones\nsecond' }, { command: 'x'.repeat(200) }, { pattern: 'refreshToken|validate' }, { pattern: '' },
  { url: 'https://example.com/docs?a=1' }, { url: 'not an address' }, { url: '' }, { query: 'rust sqlite wal' }, { todos: [1, 2, 3] }, { todos: 'x' }, { description: 'a child', prompt: 'do it\nnow' }, { prompt: '\nfirst\nsecond' },
  { a: 1, b: 'the first text value', c: 'another' }, '{"file_path":"/repo/from/json.ts"}', '{"file_path":"/repo/cut/off.ts","content":"abc', '{"command":"echo \\"hi\\"\\nnext","descr', 'not json {', '[1,2]', 'null', '"text"',
];
const SUMMARIES: unknown[] = [undefined, '', 'touch a.txt [inProgress]', 'make build [completed, exit 0]', '/repo/a.ts, /repo/b/c.ts', '/repo/a.ts [failed]', 'Reply with exactly the word hi', '{"command":"ls"}', '\nsecond line first'];

group('what a tool call reads as', () => {
  it('is what VS Code\'s chat reads it as', () => {
    let compared = 0;
    const different: string[] = [];
    for (const name of NAMES) for (const input of INPUTS) for (const summary of SUMMARIES) {
      compared++;
      // A target that is missing is shown as an empty one by VS Code's chat; the phone's is always text.
      const shown = (d: Record<string, unknown>): string => JSON.stringify({ ...d, target: d['target'] || '' });
      const a = shown(describe(name, input, summary) as unknown as Record<string, unknown>), b = shown(theirs(name, input === undefined ? undefined : JSON.parse(JSON.stringify(input)) as unknown, summary));
      if (a !== b && different.length < 10) different.push(`${String(name)}(${JSON.stringify(input)}, ${JSON.stringify(summary)}):\n  phone   ${a}\n  VS Code ${b}`);
    }
    console.log(`Tool calls: ${compared} descriptions compared with the real describe, ${different.length} different`);
    expect(different).toEqual([]);
  });

  it('reads an address\'s host as a browser reads it', () => {
    const addresses = ['https://example.com/docs', 'https://Example.COM:443/a', 'http://example.com:80/', 'http://example.com:8080/x', 'https://user:pass@host.example/path', 'https://host.example?query', 'https://host.example#part', 'http://localhost:3000', 'http://127.0.0.1:8000/x',
      'http://[::1]:3000/', 'ftp://files.example.com/a', 'ws://socket.example/', 'wss://socket.example:443/', 'https://xn--bcher-kva.example/', 'HTTPS://UPPER.EXAMPLE/', 'file:///etc/hosts', 'mailto:a@b.co', 'custom://thing/here', 'custom:thing', 'data:text/plain,hi',
      'not an address', 'example.com/path', '/relative', '', 'https://', 'http:///x', 'https://host.example:99999/', 'https://host.example:abc/', '  https://spaces.example/  ', 'https://a.example/path with space', 'javascript:alert(1)', 'https:example.com', 'https:/example.com', 'https:\\\\example.com\\x'];
    const different: string[] = [];
    for (const address of addresses) {
      let expected: unknown;
      try { expected = new URL(address).host; } catch { expected = address; }
      if (hostOf(address) !== expected) different.push(`${JSON.stringify(address)}: the phone reads ${JSON.stringify(hostOf(address))}, a browser ${JSON.stringify(expected)}`);
    }
    console.log(`Hosts of addresses: ${addresses.length} compared with what URL gives, ${different.length} different`);
    expect(different).toEqual([]);
  });

  it('describes input VS Code\'s code stops on', () => {
    // An edit that is nothing: VS Code's describe throws, and its chat drops the event.
    const odd = { file_path: 'a.ts', edits: [{ old_string: 'a', new_string: 'b' }, null] };
    expect(() => theirs('MultiEdit', odd)).toThrow();
    expect(describe('MultiEdit', odd)).toMatchObject({ verb: 'Edited', target: 'a.ts', added: 1, removed: 1 });
  });

  it('counts lines and reads input that is cut off', () => {
    expect([lineCount(''), lineCount('a'), lineCount('a\n'), lineCount('a\nb'), lineCount('a\nb\n'), lineCount(undefined)]).toEqual([0, 1, 1, 2, 2, 0]);
    expect(parseInput('{"file_path":"/a/b.ts","content":"line\\none')).toEqual({ file_path: '/a/b.ts' });
    expect(parseInput({ a: 1 })).toEqual({ a: 1 });
    expect(parseInput(null)).toEqual({});
  });
});
