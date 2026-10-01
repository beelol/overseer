import { createContext, useContext } from 'react';

import type { conversation, markdown } from '@/model';

/**
 * What a row can do, given once for the whole list. The functions are the same for as long as
 * the screen shows the same agent, so a row is drawn again only when the row itself changes.
 */
export interface RowActions {
  /** The agent the conversation is of: rows of another run are a child's. */
  readonly rootId: string;
  /** Opens a fold of steps, closes a child, and back. */
  toggle(key: string): void;
  /** Shows what a tool call was given and what it returned. */
  openTool(key: string): void;
  /** Opens the file's changes where the agent edited it. */
  openFile(run: string, path: string): void;
  answer(requestId: string, allow: boolean, message: string): void;
  /** Sends a message that failed once more, as a new message. */
  retry(requestId: string): void;
  /** Takes a failed message off the list. */
  remove(requestId: string): void;
  /** Takes back a message that waits on the phone for the turn to end. */
  cancel(requestId: string): void;
  signIn(): void;
  /** Opens another agent's conversation: the one that took over the work, or the one it came from. */
  openAgent(runId: string): void;
  markdownOf(row: conversation.MessageRow | conversation.ThinkingRow): readonly markdown.Block[];
  /** True once for a row that came live: it arrives, and a request for permission is felt. */
  arriving(key: string): boolean;
}

const nothing = (): void => undefined;

const ActionsContext = createContext<RowActions>({
  rootId: '',
  toggle: nothing,
  openTool: nothing,
  openFile: nothing,
  answer: nothing,
  retry: nothing,
  remove: nothing,
  cancel: nothing,
  signIn: nothing,
  openAgent: nothing,
  markdownOf: () => [],
  arriving: () => false,
});

export const ActionsProvider = ActionsContext.Provider;

export function useRowActions(): RowActions {
  return useContext(ActionsContext);
}
