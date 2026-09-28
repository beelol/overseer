// The plain description both sides are turned into before they are compared: what a person would
// see, row by row, and what VS Code keeps in a tooltip or a label for a screen reader.

export interface Line {
  /** How deep the row sits: 0 in a turn, one more under a fold, a tool call or a child. */
  depth: number;
  /** The turn's number, as VS Code keeps it on the turn. */
  turn: string;
  kind: 'user' | 'message' | 'thinking' | 'steps' | 'tool' | 'edit' | 'permission' | 'error' | 'child' | 'note' | 'footer';
  [field: string]: unknown;
}

export interface Description {
  /** Said above the rows, or null. */
  banner: string | null;
  /** Said under the rows while the agent works, or null. */
  working: string | null;
  lines: Line[];
}

/** White space as a person sees it: runs are one space, none at the ends. */
export const squash = (s: string | null | undefined): string => String(s ?? '').replace(/\s+/g, ' ').trim();

const escape = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
export const mark = { text: escape };
