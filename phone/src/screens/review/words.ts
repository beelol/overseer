/**
 * The sentences of the review, merge back and pull request screens that `text.TEXT` does not
 * hold. Those VS Code also says are marked with the file they stand in; they belong in
 * `phone/model/src/text.ts` with the others (reported to the owner of the model).
 */
import { text } from '@/model';

const plural = (n: number): string => (n === 1 ? '' : 's');
const files = (n: number): string => text.TEXT.chat.files(n);

export const WORDS = {
  changes: {
    title: 'Changes',
    filter: 'Filter files',
    clearFilter: 'Clear the filter',
    /** extension/src/review.js, the title of the picker. */
    compareWith: 'Compare the working tree with…',
    /** extension/src/review.js */
    branchToCompare: 'Branch to compare with',
    noBranches: 'No other branches.',
    reviewed: (done: number, all: number): string => `${done} of ${all} reviewed`,
    asOf: (age: string): string => `As of ${age}`,
    noMatch: 'No files match.',
    fold: (name: string): string => `Fold ${name}`,
    unfold: (name: string): string => `Unfold ${name}`,
  },
  file: {
    lines: (first: number, last: number): string => (first === last ? `Line ${first}` : `Lines ${first} to ${last}`),
    deletionAfter: (line: number): string => (line > 0 ? `After line ${line}` : 'At the start'),
    accept: 'Accept',
    reject: 'Reject',
    wrap: 'Wrap long lines',
    unwrap: 'Scroll long lines sideways',
    putBack: (n: number): string => (n === 1 ? 'Put this line back?' : `Put these ${n} lines back?`),
    takeOut: (n: number): string => (n === 1 ? 'Take this added line out?' : `Take these ${n} added lines out?`),
    putBackDetail: (added: number): string => (added === 0 ? '' : `The ${added} line${plural(added)} the agent wrote in their place ${added === 1 ? 'is' : 'are'} removed from the file.`),
    takeOutDetail: 'The file goes back to how it was here.',
    putBackConfirm: 'Put back',
    changedSince: 'Nothing was put back, because the file changed since.',
    changedSinceAccept: 'Not marked as reviewed, because the file changed since.',
    noChanges: 'No changes in this file for this comparison.',
    more: (n: number): string => `… ${text.grouped(n)} more characters`,
    removedLine: (line: number | null, words: string): string => `Removed, line ${line ?? ''}: ${words}`,
    addedLine: (line: number | null, words: string): string => `Added, line ${line ?? ''}: ${words}`,
    nothingNamed: 'No file was named.',
  },
  merge: {
    title: 'Merge back',
    plan: 'Plan',
    into: (branch: string, target: string): string => `${branch} → ${target}`,
    merge: 'Merge',
    repository: 'Repository',
    state: 'State',
    states: { idle: 'Not prepared', ready: 'Ready to merge', resolving: 'Conflicts to resolve', resolved: 'Conflicts resolved', merged: 'Merged' } as Readonly<Record<string, string>>,
    /** extension/src/extension.js: `Merge back is unavailable: ${plan.reason}` */
    unavailable: (reason: string): string => `Merge back is unavailable: ${reason}`,
    steps: 'What happens',
    commitFirst: (n: number, branch: string): string => `Commit ${files(n)} that ${n === 1 ? 'is' : 'are'} not committed to ${branch}.`,
    nothingToCommit: 'Everything in the worktree is committed.',
    mergeTarget: (target: string, branch: string): string => `Merge ${target} into ${branch} inside the worktree. Conflicts go back to the agent.`,
    thenReview: (target: string): string => `You see what will land, then confirm. Nothing reaches ${target} before that.`,
    finishResolving: 'Check that no conflict marks remain, then finish the merge in the worktree.',
    complete: (branch: string, target: string, repo: string): string => `Merge ${branch} into ${target} in ${repo}. The worktree and ${branch} are kept.`,
    conflicts: 'Conflicts',
    sentToAgent: 'Sent to the agent. Continue when it finishes.',
    resolveYourself: (why: string): string => `Resolve them in the worktree${why ? ` (${why})` : ''}, then continue.`,
    marksRemain: (names: string): string => `Conflict marks remain in ${names}.`,
    blocked: 'Blocked',
    lands: 'What will land',
    landsNothing: 'Nothing to show yet.',
    prepare: 'Prepare',
    resume: 'Continue',
    finish: 'Complete',
    abort: 'Abort',
    working: 'Working…',
    askComplete: (branch: string, target: string, repo: string): string => `Merge ${branch} into ${target} in ${repo}?`,
    askCompleteDetail: (n: number, target: string): string => `${files(n)} will land on ${target}. This cannot be taken back from the phone.`,
    confirmComplete: 'Complete merge back',
    askAbort: 'Abort this merge?',
    askAbortDetail: 'What was resolved so far in the worktree is lost. The agent’s own commits are kept.',
    confirmAbort: 'Abort merge',
    aborted: 'The merge was aborted. Nothing reached the target branch.',
    /** extension/src/extension.js, shortened to what a phone shows. */
    merged: (branch: string, target: string, commit: string): string => `Merged ${branch} into ${target}${commit ? ` (${commit})` : ''}. The worktree and the branch are kept.`,
    uncommitted: 'Not committed',
  },
  pr: {
    title: 'Pull request',
    plan: 'Plan',
    branch: 'Branch',
    base: 'Base',
    remote: 'Remote',
    commits: 'Commits',
    noCommits: 'None yet',
    commitFirst: (n: number, branch: string): string => `${files(n)} will be committed to ${branch} first.`,
    /** extension/src/pull-request.js: 'Open PR is unavailable.' */
    unavailable: 'Open PR is unavailable.',
    titleField: 'Title',
    bodyField: 'Description',
    bodyHint: 'Left empty, Overseer writes the task, the commits and the files.',
    whatHappens: (branch: string, remote: string, repo: string): string => `Pushes ${branch} to ${remote} and opens a pull request on ${repo}. Nothing is merged.`,
    open: 'Open',
    opening: 'Opening…',
    /** extension/src/pull-request.js: `Pull request #${pr.number} is open.` */
    isOpen: (number: number): string => (number > 0 ? `Pull request #${number} is open.` : 'The pull request is open.'),
    wasOpen: (number: number): string => (number > 0 ? `Pull request #${number} was open already.` : 'The pull request was open already.'),
    nothingMerged: 'Nothing was merged.',
    browser: 'Open in browser',
    copy: 'Copy',
    copied: 'Copied',
    address: 'Address',
  },
  queued: text.TEXT.chat.queued,
  /** Nothing is known yet and the Mac cannot be asked now. */
  whenReached: 'Shown when the Mac is reached.',
  unknownAgent: 'This agent is not on the Mac any more.',
} as const;
