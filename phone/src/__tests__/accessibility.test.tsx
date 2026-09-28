/**
 * The accessibility audit (AC-131): every control of every screen has a label for VoiceOver and
 * TalkBack, and a test id. Each screen is drawn in a state that shows its controls; the tree is
 * walked and every control is listed. With OVERSEER_WRITE_EVIDENCE=<folder> the list is written
 * there as `labels.json` and `labels.md`.
 */
import { screen } from '@testing-library/react-native';
import fs from 'node:fs';
import path from 'node:path';
import type { ReactElement } from 'react';

import { AccountsScreen } from '@/screens/AccountsScreen';
import { AgentsScreen } from '@/screens/AgentsScreen';
import { ChangesScreen } from '@/screens/ChangesScreen';
import { answerHistory, measuring, recording, rootOf } from '@/screens/conversation/testing';
import { ConversationScreen } from '@/screens/ConversationScreen';
import { FileScreen } from '@/screens/FileScreen';
import { MergeScreen } from '@/screens/MergeScreen';
import { NewAgentScreen } from '@/screens/NewAgentScreen';
import { PairScreen } from '@/screens/PairScreen';
import { PullRequestScreen } from '@/screens/PullRequestScreen';
import { comparisons, hunk, hunksOf, RUN, stateWith, workspace, WORKSPACE } from '@/screens/review/testing';
import { SettingsScreen } from '@/screens/SettingsScreen';
import { createTestApp, type TestApp, type TestAppOptions } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('@shopify/flash-list', () => ({ ...jest.requireActual('@shopify/flash-list'), ...require('@/screens/conversation/testing').measuring() }));

interface Node {
  readonly type: string;
  readonly props: Record<string, unknown>;
  readonly children: readonly (Node | string)[] | null;
}

interface Control {
  readonly screen: string;
  readonly kind: string;
  readonly testID: string;
  readonly label: string;
}

const ROLES = ['button', 'switch', 'link', 'checkbox', 'radio', 'tab', 'menuitem', 'search', 'adjustable'];

function kindOf(node: Node): string | null {
  const role = node.props['accessibilityRole'] ?? node.props['role'];
  if (typeof role === 'string' && ROLES.includes(role)) return role;
  if (node.type === 'RCTSwitch' || node.type === 'Switch') return 'switch';
  if (node.type === 'TextInput') return 'field';
  return null;
}

function walk(node: Node | string | null, name: string, out: Control[]): void {
  if (node === null || typeof node === 'string') return;
  const kind = kindOf(node);
  if (kind !== null && node.props['accessibilityElementsHidden'] !== true && node.props['importantForAccessibility'] !== 'no-hide-descendants') {
    out.push({ screen: name, kind, testID: String(node.props['testID'] ?? ''), label: String(node.props['accessibilityLabel'] ?? node.props['aria-label'] ?? '').trim() });
  }
  for (const child of node.children ?? []) walk(child, name, out);
}

const all: Control[] = [];

async function audit(name: string, ui: ReactElement, options: TestAppOptions, prepare: (app: TestApp) => void = () => undefined, params: Record<string, string> = {}): Promise<Control[]> {
  router.reset();
  router.params = params;
  const app = await createTestApp(options);
  prepare(app);
  await app.render(ui);
  await app.settle();
  await new Promise((resolve) => setTimeout(resolve, 60));
  await app.settle();
  const found: Control[] = [];
  const tree = screen.toJSON() as Node | Node[] | null;
  for (const root of Array.isArray(tree) ? tree : [tree]) walk(root, name, found);
  all.push(...found);
  return found;
}

function expectLabelled(controls: Control[]): void {
  expect(controls.length).toBeGreaterThan(0);
  expect(controls.filter((c) => c.label === '').map((c) => `${c.screen}: a ${c.kind} with the test id "${c.testID}" has no label`)).toEqual([]);
  expect(controls.filter((c) => c.testID === '').map((c) => `${c.screen}: the ${c.kind} "${c.label}" has no test id`)).toEqual([]);
}

/** What the Mac answers about an agent's changes, its merge and its pull request. */
function review(app: TestApp): void {
  const answers = app.connection.answers as Record<string, (params: { base?: string; path?: string; branch?: string }) => unknown>;
  answers['comparison.options'] = (p) => comparisons(p.branch ?? 'main');
  answers['workspace.diff'] = (p) => ({
    workspace_id: WORKSPACE, root: workspace.path, base: p.base, current_tree: 't', index_tree: 'i', head: 'h',
    changes: [{ status: 'M', path: 'src/cart.ts' }, { status: 'A', path: 'src/tax.ts' }],
    status: { branch: workspace.branch, head: 'h', staged: [], unstaged: [], untracked: [], conflicted: [] },
  });
  answers['workspace.hunks'] = (p) => hunksOf(p.path ?? '', p.base ?? '', [hunk(p.path ?? '', 12, ['  return sum;'], ['  return sum + tax;']), hunk(p.path ?? '', 30, [], ['export const tax = 0.2;'])]);
  answers['review.marks'] = () => ({ run_id: RUN, keys: [], marks: [] });
  answers['workspace.merge_plan'] = () => ({
    ok: true, state: 'ready', workspace, run_id: RUN, repo: workspace.repo_root, branch: workspace.branch, target: 'main',
    worktree_uncommitted: [], conflicts: [], source_branch: 'main', source_dirty: [], blockers: [], can_complete: true,
  });
  answers['workspace.pr_plan'] = () => ({
    ok: true, workspace, run_id: RUN, title: 'Fix the cart', prompt: 'Fix the cart total', harness: 'claude', model: 'sonnet',
    remote: 'origin', remote_url: 'https://github.com/owner/shop.git', owner: 'owner', repo: 'shop', branch: workspace.branch, target: 'main', base_ref: 'main',
    uncommitted: [], commits: ['Add the tax to the total'],
  });
}

/** What the Mac answers about its accounts. */
function accounts(app: TestApp): void {
  const answers = app.connection.answers as Record<string, (params: { id: string }) => unknown>;
  answers['account.list'] = () => ({
    accounts: [{ id: 'p-work', name: 'Work', provider: 'openai', harness_family: 'codex', harnesses: ['codex'], kind: 'fixed', removable: true }],
    providers: [{ id: 'openai', label: 'OpenAI / ChatGPT', harnesses: ['codex', 'codex-app'], available: true, sign_in: 'a device code', why: null }],
  });
  answers['profile.status'] = ({ id }) => ({ profile_id: id, installed: true, logged_in: false, method: 'chatgpt-account' });
  answers['account.usage'] = () => ({ reported: false });
}

const WORK = { id: 'p-work', name: 'Work', harness: 'codex', home: '/Users/owner/.overseer/profiles/p-work', is_system: false, created_ms: 1 };

describe('every control has a label and a test id', () => {
  const session = recording('showcase-permission');
  const run = rootOf(session);

  test('Pair with your Mac', async () => {
    expectLabelled(await audit('Pair with your Mac', <PairScreen />, { paired: false }));
  });

  test('Agents', async () => {
    expectLabelled(await audit('Agents', <AgentsScreen />, { state: session.final }));
  });

  test('Conversation', async () => {
    expectLabelled(await audit('Conversation', <ConversationScreen />, { state: session.final }, (app) => answerHistory(app.connection, session.events), { run }));
  });

  test('Changes', async () => {
    expectLabelled(await audit('Changes', <ChangesScreen />, { state: stateWith() }, review, { run: RUN }));
  });

  test("A file's changes", async () => {
    expectLabelled(await audit("A file's changes", <FileScreen />, { state: stateWith() }, review, { run: RUN, path: 'src/cart.ts' }));
  });

  test('Merge back', async () => {
    expectLabelled(await audit('Merge back', <MergeScreen />, { state: stateWith() }, review, { run: RUN }));
  });

  test('Pull request', async () => {
    expectLabelled(await audit('Pull request', <PullRequestScreen />, { state: stateWith() }, review, { run: RUN }));
  });

  test('New agent', async () => {
    expectLabelled(await audit('New agent', <NewAgentScreen />, { state: session.final }));
  });

  test('Accounts', async () => {
    expectLabelled(await audit('Accounts', <AccountsScreen />, { state: { ...stateWith(), profiles: [WORK] } as never }, accounts));
  });

  test('Settings', async () => {
    expectLabelled(await audit('Settings', <SettingsScreen />, { state: session.final }));
  });

  afterAll(() => {
    const folder = process.env.OVERSEER_WRITE_EVIDENCE;
    if (!folder) return;
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'labels.json'), `${JSON.stringify(all, null, 2)}\n`);
    const lines = ['# Every control and its label', '', `${all.length} controls on ${new Set(all.map((c) => c.screen)).size} screens, each with a label and a test id. Written by \`src/__tests__/accessibility.test.tsx\`.`, '', '| Screen | Control | Label | Test id |', '| --- | --- | --- | --- |', ...all.map((c) => `| ${c.screen} | ${c.kind} | ${c.label.replaceAll('|', '/')} | \`${c.testID}\` |`)];
    fs.writeFileSync(path.join(folder, 'labels.md'), `${lines.join('\n')}\n`);
  });
});

// `measuring` is used by the mock above; named here so the import is not dropped.
void measuring;
