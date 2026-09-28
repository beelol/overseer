/** The test id of a row: `agent.row.<key>`, with what a test id cannot hold written as "_". */
export function rowId(key: string): string {
  return `agent.row.${key.replace(/[^A-Za-z0-9._:-]/g, '_')}`;
}

/** A key of the phone's storage for a run: letters, digits, ".", "_" and "-" only. */
export function storeKey(runId: string): string {
  return runId.replace(/[^A-Za-z0-9._-]/g, '_');
}

/**
 * What an edit chip asks the file's screen for as `hunk`: the hunk the agent edited, which the
 * file's screen finds with `review.editTarget` once it has the file's changes. The row of an
 * edit names files, not hunks.
 */
export const EDITED_HUNK = 'edited';
