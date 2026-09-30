import { act, fireEvent, screen, waitFor } from '@testing-library/react-native';

import { METHOD_CLASS, type Harness, type State } from '@/protocol';
import { routes } from '@/routes';
import { NewAgentScreen } from '@/screens/NewAgentScreen';
import { choose, missing, NOTHING_CHOSEN, paramsOf, titleFor, type Choices, type Form } from '@/screens/new/form';
import { namedIn, offers, optionsOf } from '@/screens/new/options';
import { store } from '@/model';
import { FAKE_IPHONE } from '@/platform/fake';
import { createTestApp, EMPTY_STATE, type TestApp, type TestAppOptions } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

/** A recorded session of nine agents in two repositories, with two Claude accounts. */
const recorded = require('../../../model/test/fixtures/nine-agents.json') as { final: State; marks: Record<string, string> };
const NINE = recorded.final;
const WORK = recorded.marks['account'] ?? '';
const RUNNING = NINE.runs.find((run) => run.id === recorded.marks['running']);
const CLAUDE = RUNNING?.capabilities as Record<string, string>;

/** What the daemon reports of the other harnesses (daemon/src/adapters.rs `capabilities`). */
const CODEX = { model: 'supported (-m, per turn)', effort: 'supported (model_reasoning_effort, per turn)', permission_mode: 'supported (sandbox: read-only or workspace-write)', images: 'supported (-i)' };
const CODEX_APP = { model: 'supported (at start)', effort: "unsupported in Overseer's app-server transport", permission_mode: 'supported (approval policy at start)' };
const OPENCODE = { model: 'supported (-m, per turn)', effort: 'unsupported', permission_mode: 'unsupported', images: 'unsupported' };
const GENERIC = { model: 'not applicable', effort: 'not applicable', permission_mode: 'not applicable' };

const HARNESSES: Harness[] = [
  { harness: 'codex', program: '/opt/bin/codex', version: 'codex-cli 0.155.0', installed: true, capabilities: CODEX },
  { harness: 'codex-app', program: '/opt/bin/codex', version: 'codex-cli 0.155.0', installed: true, capabilities: CODEX_APP },
  { harness: 'claude', program: '/opt/bin/claude', version: '2.1.246 (Claude Code)', installed: true, capabilities: CLAUDE },
  { harness: 'opencode', program: null, version: null, installed: false, capabilities: OPENCODE },
  { harness: 'generic', program: null, version: null, installed: true, capabilities: GENERIC },
];

const ACCOUNTS = NINE.profiles.map((profile) => ({
  id: profile.id,
  name: profile.name,
  provider: profile.harness === 'claude' ? 'anthropic' : profile.harness === 'codex' ? 'openai' : 'local',
  harness_family: profile.harness,
  harnesses: profile.harness === 'codex' ? ['codex', 'codex-app'] : [profile.harness],
  kind: profile.is_system ? 'follows-app' : 'fixed',
  // How the daemon names the account (AC-235, synthetic): the work account's plan and shortened email.
  ...(profile.id === WORK ? { account: { provider: 'Claude', plan: 'Max', email: 'wor…@acme.example', default: false, name: profile.name, label: `Claude Max · wor…@acme.example · ${profile.name}`, short: 'Claude Max · wor…@acme.example' } } : {}),
}));

/** Who is signed in on the Mac: the work account and Codex; Claude's own login is not. */
const SIGNED_IN: Readonly<Record<string, boolean>> = { 'system-claude': false, 'system-codex': true, 'system-opencode': false, [WORK]: true };

const CREATED = { task: { id: 't-new' }, run: { id: 'r-new' }, workspace: { id: 'w-new' } };

async function open(options: TestAppOptions = {}, prepare: (app: TestApp) => void = () => undefined): Promise<TestApp> {
  const app = await createTestApp({ state: NINE, ...options });
  app.connection.answers['repo.known'] = () => ({
    repos: [
      { root: '/fixture/shop', name: 'shop', last_used_ms: 3, exists: true, branch: 'main', default_branch: 'main' },
      { root: '/fixture/billing-service', name: 'billing-service', last_used_ms: 2, exists: true, branch: 'release', default_branch: 'main' },
      { root: '/fixture/gone', name: 'gone', last_used_ms: 1, exists: false, branch: null, default_branch: null },
    ],
  });
  app.connection.answers['harness.list'] = () => HARNESSES;
  app.connection.answers['account.list'] = () => ({ accounts: ACCOUNTS, providers: [] });
  app.connection.answers['profile.status'] = (params: { id: string }) => ({ profile_id: params.id, installed: true, logged_in: SIGNED_IN[params.id] === true, identity: params.id === WORK ? { plan: 'max' } : null });
  app.connection.answers['task.create'] = () => CREATED;
  prepare(app);
  await app.render(<NewAgentScreen />);
  await app.settle();
  return app;
}

const kept = (app: TestApp) => app.platform.capabilities.keyValue.scope<{ last: Form; draft: string }>('new');
const valueOf = (testID: string): string => String(screen.getByTestId(testID).props.accessibilityLabel);

beforeEach(() => router.reset());

describe('the New agent form', () => {
  test('opens on the agent started last, with a signed-in account', async () => {
    await open();
    expect(screen.getByTestId('new.title')).toHaveTextContent('New agent');
    expect(valueOf('new.repo')).toBe('Repository, shop, main');
    expect(valueOf('new.agent')).toBe('Agent, Claude Code');
    // Claude's own login is signed out on the Mac: the signed-in account comes first.
    expect(valueOf('new.account')).toBe('Account, Work account, signed in · Max · wor…@acme.example');
    expect(valueOf('new.model')).toBe('Model, Default');
    expect(valueOf('new.effort')).toBe('Effort, Default');
    expect(valueOf('new.mode')).toBe('Permissions, Default');
    expect(valueOf('new.where')).toBe('Workspace, New worktree');
    expect(screen.getByTestId('new.task').props.value).toBe('');
    expect(screen.queryByTestId('new.missing')).toBeNull();
    expect(screen.queryByTestId('new.account.signin')).toBeNull();
  });

  test('offers the repositories Overseer knows, the installed agents, and the accounts signed-in first', async () => {
    await open();
    await fireEvent.press(screen.getByTestId('new.repo'));
    expect(screen.getByTestId('new.repo.shop').props.accessibilityState).toMatchObject({ selected: true });
    expect(screen.getByTestId('new.repo.billing-service').props.accessibilityLabel).toBe('billing-service, release');
    expect(screen.queryByTestId('new.repo.gone')).toBeNull();
    await fireEvent.press(screen.getByTestId('new.repo.billing-service'));
    expect(valueOf('new.repo')).toBe('Repository, billing-service, release');

    // Installed, and a phone's to start: not OpenCode (missing), not a program, not the app-server.
    await fireEvent.press(screen.getByTestId('new.agent'));
    expect(screen.getByTestId('new.agent.claude')).toBeTruthy();
    expect(screen.getByTestId('new.agent.codex')).toBeTruthy();
    for (const harness of ['opencode', 'generic', 'codex-app']) expect(screen.queryByTestId(`new.agent.${harness}`)).toBeNull();
    await fireEvent.press(screen.getByTestId('new.agent.close', { includeHiddenElements: true }));

    await fireEvent.press(screen.getByTestId('new.account'));
    const accounts = screen.getAllByTestId(/^new\.account\.(system-|p-)/).map((node) => String(node.props.accessibilityLabel));
    expect(accounts).toEqual(['Work account, signed in · Max · wor…@acme.example', "Mac's default login, not signed in"]);
  });

  test('a signed-out account says so and offers Sign in', async () => {
    const app = await open();
    await fireEvent.press(screen.getByTestId('new.account'));
    await fireEvent.press(screen.getByTestId('new.account.system-claude'));
    expect(valueOf('new.account')).toBe("Account, Mac's default login, not signed in");
    expect(screen.getByTestId('new.account.signin').props.accessibilityLabel).toBe("Sign in, Mac's default login is not signed in.");

    await fireEvent.changeText(screen.getByTestId('new.task'), 'Fix the login page');
    await fireEvent.press(screen.getByTestId('new.start'));
    expect(screen.getByTestId('new.missing')).toHaveTextContent('Choose a signed-in account.');
    expect(app.connection.calls('task.create')).toEqual([]);

    await fireEvent.press(screen.getByTestId('new.account.signin'));
    expect(router.pushed).toEqual([routes.accounts]);
  });

  test('says what is missing in one sentence, and sends nothing', async () => {
    const app = await open({ launch: FAKE_IPHONE });
    await fireEvent.press(screen.getByTestId('new.start'));
    expect(screen.getByTestId('new.missing')).toHaveTextContent('Describe the task.');
    expect(app.platform.fakes.haptics.played()).toEqual(['reject']);

    await fireEvent.changeText(screen.getByTestId('new.task'), '   ');
    expect(screen.getByTestId('new.missing')).toHaveTextContent('Describe the task.');
    await fireEvent.changeText(screen.getByTestId('new.task'), 'Fix the login page');
    expect(screen.queryByTestId('new.missing')).toBeNull();
    expect(app.connection.calls('task.create')).toEqual([]);
    expect(router.replaced).toEqual([]);
  });

  test('with no repository it says so', async () => {
    const app = await open({ state: EMPTY_STATE }, (a) => (a.connection.answers['repo.known'] = () => ({ repos: [] })));
    expect(valueOf('new.repo')).toBe('Repository, None yet');
    expect(screen.getByText('A repository appears here once an agent has worked in it on the Mac.')).toBeTruthy();
    await fireEvent.changeText(screen.getByTestId('new.task'), 'Fix the login page');
    await fireEvent.press(screen.getByTestId('new.start'));
    expect(screen.getByTestId('new.missing')).toHaveTextContent('Choose a repository.');
    expect(app.connection.calls('task.create')).toEqual([]);
  });

  test('Start sends one request with what was chosen and opens the new agent', async () => {
    const app = await open({ launch: FAKE_IPHONE });
    await fireEvent.changeText(screen.getByTestId('new.task'), 'Fix the login page\n\nThe button does nothing on a phone.');
    await fireEvent.press(screen.getByTestId('new.start'));
    await app.settle();
    expect(app.connection.calls('task.create')).toEqual([
      { repo: '/fixture/shop', harness: 'claude', prompt: 'Fix the login page\n\nThe button does nothing on a phone.', title: 'Fix the login page', workspace_mode: 'worktree', profile_id: WORK },
    ]);
    expect(router.replaced).toEqual([routes.agent('r-new')]);
    expect(router.pushed).toEqual([]);
    expect(app.platform.fakes.haptics.played()).toEqual(['confirm']);
  });

  test('every option that was chosen is sent, and nothing a phone may not send', async () => {
    const app = await open();
    const pick = async (row: string, choice: string): Promise<void> => {
      await fireEvent.press(screen.getByTestId(row));
      await fireEvent.press(screen.getByTestId(`${row}.${choice}`));
    };
    await pick('new.repo', 'billing-service');
    await pick('new.model', 'opus');
    await pick('new.effort', 'high');
    await pick('new.mode', 'plan');
    await pick('new.where', 'current');
    expect(valueOf('new.model')).toBe('Model, opus');
    expect(valueOf('new.effort')).toBe('Effort, high');
    expect(valueOf('new.mode')).toBe('Permissions, Plan only');
    expect(valueOf('new.where')).toBe('Workspace, Current checkout');

    await fireEvent.changeText(screen.getByTestId('new.task'), 'Rename the tax helper');
    await fireEvent.press(screen.getByTestId('new.start'));
    await app.settle();
    const sent = app.connection.calls('task.create') as Record<string, unknown>[];
    expect(sent).toEqual([{ repo: '/fixture/billing-service', harness: 'claude', prompt: 'Rename the tax helper', title: 'Rename the tax helper', workspace_mode: 'current', profile_id: WORK, model: 'opus', effort: 'high', permission_mode: 'plan' }]);
    // What starts a program or changes how a harness is started is the Mac's alone.
    for (const key of ['program', 'args', 'extra_args', 'approval_policy', 'unsaved', 'env']) expect(sent[0]).not.toHaveProperty(key);
    expect(METHOD_CLASS['task.create']).toBe('control');
  });

  test('another model can be typed', async () => {
    const app = await open();
    await fireEvent.press(screen.getByTestId('new.model'));
    await fireEvent.changeText(screen.getByTestId('new.model.other'), ' claude-fable-5 ');
    await fireEvent.press(screen.getByTestId('new.model.other.use'));
    expect(valueOf('new.model')).toBe('Model, claude-fable-5');
    await fireEvent.press(screen.getByTestId('new.model'));
    await fireEvent.press(screen.getByTestId('new.model.default'));
    expect(valueOf('new.model')).toBe('Model, Default');
    expect(app.connection.calls('task.create')).toEqual([]);
  });

  test('what was chosen the last time is chosen again', async () => {
    const first = await open();
    await fireEvent.press(screen.getByTestId('new.agent'));
    await fireEvent.press(screen.getByTestId('new.agent.codex'));
    expect(valueOf('new.account')).toBe("Account, Mac's default login, signed in");
    await fireEvent.press(screen.getByTestId('new.mode'));
    await fireEvent.press(screen.getByTestId('new.mode.read-only'));
    await fireEvent.press(screen.getByTestId('new.where'));
    await fireEvent.press(screen.getByTestId('new.where.current'));
    await fireEvent.changeText(screen.getByTestId('new.task'), 'List the files');
    await fireEvent.press(screen.getByTestId('new.start'));
    await first.settle();
    const last = kept(first).get('last');
    expect(last).toEqual({ repo: '/fixture/shop', harness: 'codex', account: 'system-codex', model: '', effort: '', mode: 'read-only', where: 'current' });
    screen.unmount();

    // The next launch: another app, the same storage.
    await open({}, (app) => last && kept(app).set('last', last));
    expect(valueOf('new.agent')).toBe('Agent, Codex');
    expect(valueOf('new.account')).toBe("Account, Mac's default login, signed in");
    expect(valueOf('new.mode')).toBe('Permissions, Read only');
    expect(valueOf('new.where')).toBe('Workspace, Current checkout');
    expect(screen.getByTestId('new.task').props.value).toBe('');
  });

  test('a task that was typed and not started is still there the next time', async () => {
    const first = await open();
    await fireEvent.changeText(screen.getByTestId('new.task'), 'Half a thought');
    expect(kept(first).get('draft')).toBe('Half a thought');
    screen.unmount();
    await open({}, (app) => kept(app).set('draft', 'Half a thought'));
    expect(screen.getByTestId('new.task').props.value).toBe('Half a thought');
  });

  test('an agent offers only what it can be told', async () => {
    await open({}, (app) => (app.connection.answers['harness.list'] = () => HARNESSES.map((h) => ({ ...h, installed: true, program: '/opt/bin/x' }))));
    // Claude Code: model, effort and the permission modes the daemon names.
    await fireEvent.press(screen.getByTestId('new.mode'));
    expect(screen.getAllByTestId(/^new\.mode\.(?!close)[^.]+$/, { includeHiddenElements: true }).map((node) => node.props.accessibilityLabel)).toEqual(['Default', 'Ask first', 'Accept edits', 'Plan only', 'Auto']);
    await fireEvent.press(screen.getByTestId('new.mode.close', { includeHiddenElements: true }));
    await fireEvent.press(screen.getByTestId('new.effort'));
    expect(screen.getAllByTestId(/^new\.effort\.(?!close)[^.]+$/, { includeHiddenElements: true }).map((node) => node.props.accessibilityLabel)).toEqual(['Default', 'low', 'medium', 'high', 'xhigh', 'max']);
    await fireEvent.press(screen.getByTestId('new.effort.close', { includeHiddenElements: true }));

    // Codex: its own efforts and its two sandboxes.
    await fireEvent.press(screen.getByTestId('new.agent'));
    await fireEvent.press(screen.getByTestId('new.agent.codex'));
    await fireEvent.press(screen.getByTestId('new.mode'));
    expect(screen.getAllByTestId(/^new\.mode\.(?!close)[^.]+$/, { includeHiddenElements: true }).map((node) => node.props.accessibilityLabel)).toEqual(['Default', 'Can edit', 'Read only']);
    await fireEvent.press(screen.getByTestId('new.mode.close', { includeHiddenElements: true }));

    // OpenCode: a model, and nothing else.
    await fireEvent.press(screen.getByTestId('new.agent'));
    await fireEvent.press(screen.getByTestId('new.agent.opencode'));
    expect(valueOf('new.agent')).toBe('Agent, OpenCode');
    expect(screen.getByTestId('new.model')).toBeTruthy();
    expect(screen.queryByTestId('new.effort')).toBeNull();
    expect(screen.queryByTestId('new.mode')).toBeNull();
    expect(screen.getByTestId('new.where')).toBeTruthy();
  });

  test('an option the next agent does not have is not sent', async () => {
    const app = await open({}, (a) => (a.connection.answers['harness.list'] = () => HARNESSES.map((h) => ({ ...h, installed: true, program: '/opt/bin/x' }))));
    await fireEvent.press(screen.getByTestId('new.effort'));
    await fireEvent.press(screen.getByTestId('new.effort.max'));
    await fireEvent.press(screen.getByTestId('new.mode'));
    await fireEvent.press(screen.getByTestId('new.mode.plan'));
    await fireEvent.press(screen.getByTestId('new.agent'));
    await fireEvent.press(screen.getByTestId('new.agent.codex'));
    // Codex has no "max" effort and no "plan" mode.
    expect(valueOf('new.effort')).toBe('Effort, Default');
    expect(valueOf('new.mode')).toBe('Permissions, Default');
    await fireEvent.changeText(screen.getByTestId('new.task'), 'List the files');
    await fireEvent.press(screen.getByTestId('new.start'));
    await app.settle();
    expect(app.connection.calls('task.create')).toEqual([{ repo: '/fixture/shop', harness: 'codex', prompt: 'List the files', title: 'List the files', workspace_mode: 'worktree', profile_id: 'system-codex' }]);
  });

  test('what the Mac refuses is said, and the task stays', async () => {
    const app = await open({}, (a) => {
      a.connection.answers['task.create'] = () => {
        throw new Error('the repository has no commits');
      };
    });
    await fireEvent.changeText(screen.getByTestId('new.task'), 'Fix the login page');
    await fireEvent.press(screen.getByTestId('new.start'));
    await app.settle();
    expect(screen.getByTestId('new.error')).toHaveTextContent('Not started. the repository has no commits');
    expect(screen.getByTestId('new.task').props.value).toBe('Fix the login page');
    expect(kept(app).get('draft')).toBe('Fix the login page');
    expect(router.replaced).toEqual([]);
    expect(screen.getByTestId('new.start').props.accessibilityState).toMatchObject({ disabled: false });
  });
});

describe('while the Mac is away', () => {
  test('Start queues the agent, says so, sends it once, and opens it when the Mac is back', async () => {
    const app = await open();
    let answer: (created: unknown) => void = () => undefined;
    app.connection.answers['task.create'] = () => new Promise((resolve) => (answer = resolve));
    await act(async () => app.connection.go('unreachable'));
    expect(screen.queryByTestId('new.queued')).toBeNull();

    await fireEvent.changeText(screen.getByTestId('new.task'), 'Fix the login page');
    await fireEvent.press(screen.getByTestId('new.start'));
    expect(screen.getByTestId('new.queued')).toHaveTextContent('Queued. It starts when the Mac is back.');
    expect(screen.getByTestId('new.start')).toHaveTextContent('Start');
    expect(screen.getByTestId('new.start').props.accessibilityState).toMatchObject({ disabled: true });
    expect(router.replaced).toEqual([]);

    // Pressed again, nothing more is sent.
    await fireEvent.press(screen.getByTestId('new.start'));
    expect(app.connection.calls('task.create')).toHaveLength(1);

    await act(async () => app.connection.go('online'));
    expect(screen.queryByTestId('new.queued')).toBeNull();
    expect(screen.getByTestId('new.start')).toHaveTextContent('Starting…');
    await act(async () => answer(CREATED));
    expect(router.replaced).toEqual([routes.agent('r-new')]);
    expect(app.connection.calls('task.create')).toHaveLength(1);
  });

  test('an agent queued earlier is still said to be queued', async () => {
    const app = await open({ connection: 'unreachable' });
    await act(async () => app.connection.setOutbox([{ requestId: 'request-1', method: 'task.create', params: { repo: '/fixture/shop', harness: 'claude', prompt: 'Fix the login page' }, state: 'queued', createdAt: 1, attempts: 0, firstSentAt: null }]));
    expect(screen.getByTestId('new.queued')).toHaveTextContent('Queued. It starts when the Mac is back.');
    await act(async () =>
      app.connection.setOutbox([
        { requestId: 'request-1', method: 'task.create', params: { repo: '/fixture/shop', harness: 'claude', prompt: 'Fix the login page' }, state: 'queued', createdAt: 1, attempts: 0, firstSentAt: null },
        { requestId: 'request-2', method: 'task.create', params: { repo: '/fixture/shop', harness: 'claude', prompt: 'And the logout page' }, state: 'queued', createdAt: 2, attempts: 0, firstSentAt: null },
      ]),
    );
    expect(screen.getByTestId('new.queued')).toHaveTextContent('2 agents queued. They start when the Mac is back.');
    expect(screen.getByTestId('new.start').props.accessibilityState).toMatchObject({ disabled: false });
  });

  test('the form opens on what the Mac said the last time', async () => {
    const first = await open();
    await waitFor(() => expect(valueOf('new.account')).toBe('Account, Work account, signed in · Max · wor…@acme.example'));
    const offered = first.platform.fakes.keyValue.items.get('new.offered');
    screen.unmount();

    const app = await createTestApp({ state: NINE, connection: 'unreachable' });
    if (offered) app.platform.fakes.keyValue.items.set('new.offered', offered);
    await app.render(<NewAgentScreen />);
    expect(valueOf('new.repo')).toBe('Repository, shop, main');
    await fireEvent.press(screen.getByTestId('new.agent'));
    expect(screen.getByTestId('new.agent.codex')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('new.agent.claude'));
    expect(valueOf('new.account')).toBe('Account, Work account, signed in · Max · wor…@acme.example');
    expect(app.connection.asked).toEqual([]);
  });

  test('with nothing from the Mac yet, the form offers what the agents the phone knows show', async () => {
    const app = await createTestApp({ state: NINE });
    await app.render(<NewAgentScreen />);
    await app.settle();
    // Nothing answers: the repositories, the agent and the accounts are those of the state.
    expect(app.connection.calls('task.create')).toEqual([]);
    expect(valueOf('new.repo')).toBe('Repository, shop');
    expect(valueOf('new.agent')).toBe('Agent, Claude Code');
    expect(valueOf('new.account')).toBe("Account, Mac's default login");
    expect(valueOf('new.mode')).toBe('Permissions, Default');
    await fireEvent.press(screen.getByTestId('new.repo'));
    expect(screen.getByTestId('new.repo.billing-service')).toBeTruthy();
  });
});

describe('a phone that may only watch', () => {
  test('has no form and no Start', async () => {
    const app = await open({ scope: 'watch' });
    expect(screen.getByTestId('watch.line')).toHaveTextContent('This phone may watch. Change it on the Mac.');
    expect(screen.queryByTestId('new.start')).toBeNull();
    expect(screen.queryByTestId('new.task')).toBeNull();
    expect(screen.queryByTestId('new.repo')).toBeNull();
    expect(app.connection.asked.map((a) => a.method)).toEqual(['state']);
  });
});

describe('the form, as data', () => {
  const choices: Choices = {
    repos: [{ root: '/r/one', name: 'one', branch: 'main' }],
    harnesses: [{ harness: 'claude', label: 'Claude Code', version: null, options: optionsOf('claude', CLAUDE) }],
    accounts: [
      { id: 'a', name: 'A', harnesses: ['claude'], signedIn: false, plan: null },
      { id: 'b', name: 'B', harnesses: ['claude'], signedIn: true, plan: null },
      { id: 'c', name: 'C', harnesses: ['codex'], signedIn: true, plan: null },
    ],
  };

  test('reads what an agent can be told from what the daemon reports', () => {
    expect(offers('supported (-m, per turn)')).toBe(true);
    expect(offers('unsupported')).toBe(false);
    expect(offers('not applicable')).toBe(false);
    expect(offers(undefined)).toBe(false);
    expect(namedIn(CLAUDE['permission_mode'])).toEqual(['acceptEdits', 'plan', 'auto', 'manual']);
    expect(namedIn(CODEX.permission_mode)).toEqual(['read-only', 'workspace-write']);
    expect(namedIn(CODEX_APP.permission_mode)).toEqual([]);
    expect(optionsOf('claude', CLAUDE)).toEqual({ models: ['sonnet', 'opus', 'haiku'], efforts: ['low', 'medium', 'high', 'xhigh', 'max'], modes: ['manual', 'acceptEdits', 'plan', 'auto'] });
    expect(optionsOf('codex', CODEX).modes).toEqual(['workspace-write', 'read-only']);
    expect(optionsOf('opencode', OPENCODE)).toEqual({ models: [], efforts: null, modes: null });
    expect(optionsOf('generic', GENERIC)).toEqual({ models: null, efforts: null, modes: null });
    // A mode the daemon names and VS Code does not know yet is offered after the known ones.
    expect(optionsOf('claude', { permission_mode: 'supported (--permission-mode: plan, careful)' }).modes).toEqual(['plan', 'careful']);
  });

  test('chooses what the Mac still offers, and says what is missing in order', () => {
    const state = store.load(EMPTY_STATE);
    const none = choose(NOTHING_CHOSEN, { repos: [], harnesses: [], accounts: [] }, state);
    expect(missing(none, 'x')).toBe('Choose a repository.');
    expect(missing(choose(NOTHING_CHOSEN, { ...choices, harnesses: [] }, state), 'x')).toBe('Choose an agent.');
    expect(missing(choose(NOTHING_CHOSEN, { ...choices, accounts: [] }, state), 'x')).toBe('Choose an account.');

    const chosen = choose({ ...NOTHING_CHOSEN, account: 'gone', effort: 'huge', mode: 'plan', model: 'opus' }, choices, state);
    expect(chosen.accounts.map((a) => a.id)).toEqual(['b', 'a']);
    expect(chosen.account?.id).toBe('b');
    expect(chosen).toMatchObject({ effort: '', mode: 'plan', model: 'opus', where: 'worktree' });
    expect(missing(chosen, '')).toBe('Describe the task.');
    expect(missing(chosen, 'Do it')).toBeNull();
    expect(missing(choose({ ...NOTHING_CHOSEN, account: 'a' }, choices, state), 'Do it')).toBe('Choose a signed-in account.');
    expect(paramsOf(none, 'Do it')).toBeNull();
  });

  test('makes the title VS Code makes', () => {
    expect(titleFor('\n  Fix   the login page  \nmore')).toBe('Fix the login page');
    expect(titleFor('')).toBe('');
    const long = 'Rewrite the billing reconciliation job so that it handles partial refunds correctly';
    expect(titleFor(long)).toBe('Rewrite the billing reconciliation job so that it handles…');
    expect(titleFor(long).length).toBeLessThanOrEqual(61);
  });
});
