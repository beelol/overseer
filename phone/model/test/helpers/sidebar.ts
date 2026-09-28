// VS Code's side of the agents list: the extension's real views.js, run in Node.
//
// views.js asks for the `vscode` module, which exists only inside VS Code. It is given a small
// stand-in with the few things the file uses (tree items, icons, colours, addresses, an event
// emitter): plain objects that keep what they are given, so nothing of the list's logic is in
// the stand-in. The list of agents that need the owner is a function inside extension.js
// (`attention`, within `activate`); its source is taken from the file as it stands and run with
// the same state, so it is the real one too. The tree is then walked the way VS Code walks it
// (getChildren, then the children of every open row) and each row is read from its tree item.
import Module, { createRequire } from 'node:module';
import path from 'node:path';
import { repoRoot } from './fixtures.ts';
import { functionSource } from './source.ts';
import type { State } from '../../src/types.ts';

class ThemeColor { id: string; constructor(id: string) { this.id = id; } }
class ThemeIcon { id: string; color: ThemeColor | undefined; constructor(id: string, color?: ThemeColor) { this.id = id; this.color = color; } }
class MarkdownString { value: string; constructor(value: string) { this.value = value; } }
class TreeItem {
  label: string; collapsibleState: number;
  id?: string; description?: string; tooltip?: string | MarkdownString; iconPath?: unknown; resourceUri?: { scheme: string; path: string }; contextValue?: string;
  accessibilityInformation?: { label: string };
  constructor(label: string, collapsibleState = 0) { this.label = label; this.collapsibleState = collapsibleState; }
}
class EventEmitter<T> {
  private listeners: Array<(value: T) => void> = [];
  event = (listener: (value: T) => void): { dispose(): void } => { this.listeners.push(listener); return { dispose: () => {} }; };
  fire(value: T): void { for (const l of this.listeners) l(value); }
}
const stub = {
  ThemeColor, ThemeIcon, MarkdownString, TreeItem, EventEmitter,
  TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
  Uri: {
    from: (parts: { scheme: string; path: string }) => ({ ...parts }),
    joinPath: (base: { path: string }, ...more: string[]) => ({ scheme: 'file', path: [base.path, ...more].join('/') }),
  },
};

type Loader = { _load(request: string, ...rest: unknown[]): unknown };
const require = createRequire(import.meta.url);
const original = (Module as unknown as Loader)._load;
(Module as unknown as Loader)._load = function (this: unknown, request: string, ...rest: unknown[]): unknown {
  return request === 'vscode' ? stub : original.call(this, request, ...rest);
};
const views = require(path.join(repoRoot, 'extension/src/views.js')) as {
  Model: new (client: unknown) => { state: State; error?: string; onDidChange: unknown; run(id: string): unknown };
  AgentsProvider: new (model: unknown, memento: unknown, uri: unknown, handlers: unknown) => SideBar;
};
(Module as unknown as Loader)._load = original;

interface Node { item: TreeItem; run?: { id: string }; task?: { id: string }; repo?: string }
interface SideBar {
  filter: { query: string; taskIds: Set<string> } | undefined;
  showArchived: boolean;
  getChildren(node?: Node): Node[];
  decorations: { provideFileDecoration(uri: unknown): { badge: string; color: ThemeColor | undefined; tooltip: string } | undefined };
}

export interface SideBarRow {
  id: string;
  depth: number;
  label: string;
  description: string;
  tooltip: string;
  accessibilityLabel: string;
  context: string;
  logo: string | null;
  icon: string | null;
  badge: string | null;
  statusText: string | null;
  emphasized: boolean;
  tone: string | null;
  expandable: boolean;
  expanded: boolean;
}

export interface SideBarOptions {
  seen?: Record<string, number>;
  changed?: Record<string, number>;
  pinned?: string[];
  collapsed?: string[];
  showArchived?: boolean;
  filter?: { query: string; taskIds: string[] };
  error?: string;
}

const tone = (color: ThemeColor | undefined): string | null => (color ? color.id.replace(/^charts\./, '').replace('descriptionForeground', 'quiet') : null);

/** The rows of VS Code's agents list for a state, read from the real tree. `Date.now()` is the time the test set. */
export function sideBar(state: State, options: SideBarOptions = {}): { rows: SideBarRow[]; needs: Array<{ run_id: string; rank: number; label: string; detail: string }> } {
  const model = new views.Model({ request: async () => state });
  model.state = state;
  if (options.error) model.error = options.error;
  const reviewed = new Map(Object.entries(options.seen ?? {}));
  const changedRuns = new Map(Object.entries(options.changed ?? {}));
  const archivedTasks = (): string[] => (model.state.tasks || []).filter(t => t.archived_ms).map(t => t.id);
  const attention = new Function('model', 'reviewed', 'changedRuns', 'archivedTasks', 'checkChanged', `${functionSource('extension/src/extension.js', 'attention')}; return attention;`)(model, reviewed, changedRuns, archivedTasks, () => {}) as () => Array<{ run_id: string; rank: number; label: string; detail: string }>;
  const memento = { get: (_key: string, fallback: unknown) => (options.collapsed ? options.collapsed : fallback), update: () => {} };
  const provider = new views.AgentsProvider(model, memento, { path: '/extension' }, { attention, pinned: () => options.pinned ?? [] });
  provider.showArchived = !!options.showArchived;
  provider.filter = options.filter ? { query: options.filter.query, taskIds: new Set(options.filter.taskIds) } : undefined;
  const rows: SideBarRow[] = [];
  const walk = (node: Node | undefined, depth: number): void => {
    for (const child of provider.getChildren(node)) {
      const item = child.item;
      const icon = item.iconPath;
      const picture = icon instanceof ThemeIcon ? { logo: null, icon: icon.id, tone: tone(icon.color) } : icon ? { logo: path.basename((icon as { light: { path: string } }).light.path).replace(/-light\.svg$/, ''), icon: null, tone: null } : { logo: null, icon: null, tone: null };
      const decoration = item.resourceUri ? provider.decorations.provideFileDecoration(item.resourceUri) : undefined;
      const tip = item.tooltip instanceof MarkdownString ? item.tooltip.value.replace(/\*\*/g, '').replace(/\n\n/g, '\n') : item.tooltip ?? '';
      rows.push({
        id: item.id ?? '', depth, label: item.label, description: item.description ?? '', tooltip: tip, accessibilityLabel: item.accessibilityInformation?.label ?? item.label, context: item.contextValue ?? '',
        logo: picture.logo, icon: picture.icon, badge: decoration?.badge ?? null, statusText: decoration?.tooltip ?? null, emphasized: !!decoration?.color, tone: decoration?.color ? tone(decoration.color) : picture.tone,
        expandable: item.collapsibleState !== 0, expanded: item.collapsibleState === 2,
      });
      if (item.collapsibleState === 2) walk(child, depth + 1);
    }
  };
  walk(undefined, 0);
  return { rows, needs: attention() };
}
