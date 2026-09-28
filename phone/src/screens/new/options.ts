import { record } from '@/model';

/**
 * What can be chosen for an agent besides its account: model, effort, permission mode.
 *
 * Whether an agent has an option is what the daemon reports for its harness (`capabilities`,
 * each "supported …", "unsupported …" or "not applicable"). The permission modes are read from
 * the capability's own text where it names them. The daemon names no models and no efforts, so
 * those lists are VS Code's (extension/media/prompt-tools.js OPTIONS, composer.js MODELS).
 */
export interface HarnessOptions {
  /** Models to choose from; `null` when the agent takes no model. Another model can be typed. */
  readonly models: readonly string[] | null;
  /** `null` when the agent has no effort to choose. */
  readonly efforts: readonly string[] | null;
  /** `null` when the agent has no permission mode to choose. */
  readonly modes: readonly string[] | null;
}

const KNOWN: Readonly<Record<string, { readonly models: readonly string[]; readonly efforts: readonly string[]; readonly modes: readonly string[] }>> = {
  claude: { models: ['sonnet', 'opus', 'haiku'], efforts: ['low', 'medium', 'high', 'xhigh', 'max'], modes: ['manual', 'acceptEdits', 'plan', 'auto'] },
  codex: { models: ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'], efforts: ['minimal', 'low', 'medium', 'high', 'xhigh'], modes: ['workspace-write', 'read-only'] },
  'codex-app': { models: ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'], efforts: [], modes: [] },
  opencode: { models: [], efforts: [], modes: [] },
};

/** True for a capability the daemon reports as supported. */
export function offers(capability: unknown): boolean {
  return typeof capability === 'string' && capability.startsWith('supported');
}

/**
 * The names a capability lists after a colon inside its parentheses:
 * "supported (--permission-mode: acceptEdits, plan, auto, manual)" names four,
 * "supported (sandbox: read-only or workspace-write)" two, "supported (at start)" none.
 */
export function namedIn(capability: unknown): readonly string[] {
  if (typeof capability !== 'string') return [];
  const inside = /\(([^)]*)\)/.exec(capability)?.[1] ?? '';
  const colon = inside.lastIndexOf(':');
  if (colon < 0) return [];
  return inside
    .slice(colon + 1)
    .split(/,|\bor\b/)
    .map((name) => name.trim())
    .filter((name) => /^[A-Za-z][A-Za-z0-9_-]*$/.test(name));
}

/** The options of a harness, from what the daemon reports of it. */
export function optionsOf(harness: string, capabilities: unknown): HarnessOptions {
  const reported = record(capabilities);
  const known = KNOWN[harness] ?? { models: [], efforts: [], modes: [] };
  const named = namedIn(reported['permission_mode']);
  // VS Code's order first, then what the daemon names and VS Code does not know yet.
  const modes = named.length > 0 ? [...known.modes.filter((mode) => named.includes(mode)), ...named.filter((mode) => !known.modes.includes(mode))] : known.modes;
  return {
    models: offers(reported['model']) ? known.models : null,
    efforts: offers(reported['effort']) && known.efforts.length > 0 ? known.efforts : null,
    modes: offers(reported['permission_mode']) && modes.length > 0 ? modes : null,
  };
}
