// What a tool call reads as: its icon, verb and target, from its name and input. The same rules
// as `describe` in extension/media/conversation.js, line for line, so "Read README.md" on the
// Mac is "Read README.md" on the phone.

import { plainTool } from './plain.ts';
import { basename, firstLine, TEXT } from './text.ts';

export interface ToolDescription {
  /** The codicon's name in VS Code; the app has its own picture for each. */
  icon: string;
  /** What it did: "Read", "Ran". */
  verb: string;
  /** What it is doing or asks to do: "Run", "Create". Missing where the verb serves for both. */
  pending?: string;
  /** What it did it to, short: a file's name, the first line of a command. */
  target: string;
  /** The same in full: the whole path, the whole command. VS Code keeps it in the tooltip. */
  full?: unknown;
  /** The target is code: shown in the fixed-width font. */
  code?: boolean;
  added?: number;
  removed?: number;
}

const FIELDS = ['file_path', 'path', 'command', 'pattern', 'description', 'url', 'query', 'prompt', 'notebook_path'];

/** A tool's input as a record: itself, or parsed from text, or what can be read from cut-off JSON. */
export function parseInput(value: unknown): Record<string, unknown> {
  if (value && typeof value === 'object') return value as Record<string, unknown>;
  const s = String(value || '');
  try {
    const j: unknown = JSON.parse(s);
    if (j && typeof j === 'object') return j as Record<string, unknown>;
  } catch { /* cut off, or not JSON */ }
  const out: Record<string, unknown> = {};
  for (const k of FIELDS) {
    const m = new RegExp(`"${k}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`).exec(s);
    if (m) out[k] = (m[1] as string).replace(/\\n/g, '\n').replace(/\\"/g, '"');
  }
  return out;
}

/** Lines of a text, not counting an empty one after a final line end. */
export function lineCount(t: unknown): number {
  return t ? String(t).split('\n').length - (String(t).endsWith('\n') ? 1 : 0) : 0;
}

const SPECIAL: Readonly<Record<string, string>> = { http: '80', https: '443', ws: '80', wss: '443', ftp: '21' };

/**
 * The host of an address ("example.com", "localhost:3000"), or the value itself when it is not an
 * address. VS Code asks the browser (`new URL(u).host`); a phone has no such thing everywhere, so
 * this reads the address itself. test/describe.test.ts compares the two.
 */
export function hostOf(u: unknown): unknown {
  const s = String(u).replace(/^[\u0000-\u0020]+|[\u0000-\u0020]+$/g, '').replace(/[\t\n\r]/g, '');
  const m = /^([a-zA-Z][a-zA-Z0-9+.-]*):([\s\S]*)$/.exec(s);
  if (!m) return u;
  const scheme = (m[1] as string).toLowerCase();
  let rest = m[2] as string;
  const special = Object.hasOwn(SPECIAL, scheme);
  if (scheme === 'file') return /^[/\\]{2}([^/\\?#]*)/.exec(rest)?.[1]?.toLowerCase().replace(/^localhost$/, '') ?? '';
  if (special) rest = rest.replace(/^[/\\]*/, '');
  else if (rest.startsWith('//')) rest = rest.slice(2);
  else return '';
  const authority = (special ? /^[^/\\?#]*/ : /^[^/?#]*/).exec(rest)?.[0] ?? '';
  let host = authority.slice(authority.lastIndexOf('@') + 1);
  let port = '';
  const colon = /:(\d*)$/.exec(host);
  if (colon && !host.endsWith(']')) { port = colon[1] as string; host = host.slice(0, colon.index); }
  else if (/:[^\]]*$/.test(host) && !host.endsWith(']')) return u;
  if (special) {
    if (!host) return u;
    host = host.toLowerCase();
    if (/[\s<>^|%#/?@[\\\]]/.test(host.replace(/^\[[0-9a-f:.]*\]$/, ''))) return u;
  }
  if (port !== '') {
    const n = Number(port);
    if (n > 65535) return u;
    port = String(n);
  }
  return port !== '' && port !== SPECIAL[scheme] ? `${host}:${port}` : host;
}

const withoutStatus = (summary: unknown): string => String(summary || '').replace(/\s*\[[^\]]*\]\s*$/, '');
const str = (v: unknown): string => (v === undefined || v === null ? '' : String(v));

export function describe(name: unknown, input: unknown, summary?: unknown): ToolDescription {
  const i = parseInput(input !== undefined && input !== null ? input : summary);
  const file = i['file_path'] || i['notebook_path'] || i['path'];
  const n = String(name || 'tool');
  const t = TEXT.tool;
  switch (n) {
    case 'Read': return { icon: 'file', verb: t.read, target: basename(file), full: file };
    case 'Write': return { icon: 'new-file', verb: t.created, pending: t.create, target: basename(file), full: file, added: lineCount(i['content']) };
    case 'Edit': case 'MultiEdit': case 'NotebookEdit': {
      const edits: Array<Record<string, unknown>> = Array.isArray(i['edits']) ? (i['edits'] as Array<Record<string, unknown>>) : [i];
      return {
        icon: 'edit', verb: t.edited, pending: t.edit, target: basename(file), full: file,
        added: edits.reduce((a, e) => a + lineCount((e && (e['new_string'] || e['new_source']))), 0),
        removed: edits.reduce((a, e) => a + lineCount(e && e['old_string']), 0),
      };
    }
    case 'Grep': return { icon: 'search', verb: t.searched, target: i['pattern'] ? t.quoted(str(i['pattern'])) : '', full: i['pattern'] };
    case 'Glob': return { icon: 'search', verb: t.foundFiles, target: str(i['pattern'] || ''), full: i['pattern'] };
    case 'LS': return { icon: 'folder', verb: t.listed, target: basename(i['path']), full: i['path'] };
    case 'Bash': case 'shell': case 'command': case 'commandExecution': {
      const cmd = i['command'] || withoutStatus(summary);
      return { icon: 'terminal', verb: t.ran, pending: t.run, target: firstLine(cmd, 80), full: i['description'] ? `${str(i['description'])}\n${str(cmd)}` : cmd, code: true };
    }
    case 'apply_patch': case 'fileChange': return { icon: 'edit', verb: t.edited, target: withoutStatus(summary).split(', ').map(basename).join(', '), full: summary };
    case 'WebFetch': return { icon: 'globe', verb: t.fetched, target: str(hostOf(i['url'])), full: i['url'] };
    case 'WebSearch': case 'web_search': case 'webSearch': return { icon: 'globe', verb: t.searchedWeb, target: i['query'] ? t.quoted(str(i['query'])) : '', full: i['query'] };
    case 'TodoWrite': return { icon: 'checklist', verb: t.updatedPlan, target: Array.isArray(i['todos']) ? t.items(i['todos'].length) : '' };
    case 'Agent': case 'Task': case 'task': return { icon: 'hubot', verb: t.delegated, target: str(i['description'] || firstLine(i['prompt'], 80)), full: i['prompt'] };
    default:
      if (/^collab:spawn_agent|^spawn_agent/.test(n)) return { icon: 'hubot', verb: t.delegated, target: firstLine(summary, 80), full: summary };
      if (/^collab:/.test(n)) return { icon: 'watch', verb: n.replace('collab:', '').replace(/_/g, ' ').replace(/^\w/, c => c.toUpperCase()), target: '' };
      // A tool's internal name (mcp__server__tool) in words (AC-245).
      if (/mcp/i.test(n)) return { icon: 'plug', verb: t.used, target: plainTool(n), full: summary };
      return { icon: 'tools', verb: n, target: firstLine(Object.values(i).find(v => typeof v === 'string') || '', 60), full: summary };
  }
}

/** Tools that start a sub-agent: its conversation nests under the call. */
export const SPAWN_TOOLS = /^(Agent|Task|task|collab:spawn_agent|spawn_agent)$/;
