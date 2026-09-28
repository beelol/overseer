import { record, type Run } from '@/model';
import type { IconName } from '@/ui';

export interface ModeChoice {
  readonly value: string;
  readonly label: string;
  readonly icon: IconName;
}

/** What a message to an agent may choose, besides its words. Empty lists are not offered. */
export interface TurnChoices {
  readonly models: readonly string[];
  readonly efforts: readonly string[];
  readonly modes: readonly ModeChoice[];
  readonly images: boolean;
}

interface HarnessChoices {
  readonly models: readonly string[];
  readonly efforts: readonly string[];
  readonly modes: readonly ModeChoice[];
}

// Copied from VS Code's composer (extension/media/prompt-tools.js, OPTIONS): the daemon says
// whether an agent takes a model, an effort or a permission mode, not which ones there are.
const BY_HARNESS: Readonly<Record<string, HarnessChoices>> = {
  claude: {
    models: ['sonnet', 'opus', 'haiku'],
    efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
    modes: [
      { value: 'manual', label: 'Ask first', icon: 'shield' },
      { value: 'acceptEdits', label: 'Accept edits', icon: 'edit' },
      { value: 'plan', label: 'Plan only', icon: 'checklist' },
      { value: 'auto', label: 'Auto', icon: 'rocket' },
    ],
  },
  codex: {
    models: ['gpt-5.6-luna', 'gpt-5.6', 'gpt-5.6-codex'],
    efforts: ['minimal', 'low', 'medium', 'high', 'xhigh'],
    modes: [
      { value: 'workspace-write', label: 'Can edit', icon: 'edit' },
      { value: 'read-only', label: 'Read only', icon: 'eye' },
    ],
  },
  opencode: { models: [], efforts: [], modes: [] },
};

const NONE: TurnChoices = Object.freeze({ models: [], efforts: [], modes: [], images: false });

/** True when the daemon reports the ability as supported ("supported (--model, per turn)"). */
export function supports(run: Pick<Run, 'capabilities'> | undefined, ability: string): boolean {
  return String(record(run?.capabilities)[ability] ?? '').startsWith('supported');
}

/**
 * What the composer offers for this agent: a choice is there when the daemon reports the
 * ability as supported for the run and VS Code knows values for its harness.
 */
export function choicesFor(run: Pick<Run, 'harness' | 'capabilities'> | undefined): TurnChoices {
  if (!run) return NONE;
  const known = Object.hasOwn(BY_HARNESS, run.harness) ? BY_HARNESS[run.harness] : undefined;
  return {
    models: known && supports(run, 'model') ? known.models : [],
    efforts: known && supports(run, 'effort') ? known.efforts : [],
    modes: known && supports(run, 'permission_mode') ? known.modes : [],
    images: supports(run, 'images'),
  };
}
