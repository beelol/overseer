import { memo, useState } from 'react';

import type { conversation } from '@/model';

import { useRowActions } from './actions';
import type { Answer } from './answers';
import { rowId } from './ids';
import { Frame } from './rows/Frame';
import { PermissionCard } from './rows/PermissionCard';
import { Child, Edit, Steps, Tool } from './rows/StepRows';
import { Failure, Footer, Message, Note, Thinking } from './rows/TextRows';
import { UserBubble } from './rows/UserBubble';

export interface RowProps {
  readonly row: conversation.Row;
  /** A fold of steps that was opened; a child that was not closed. */
  readonly open: boolean;
  /** The depths of the children that hold the row. */
  readonly lines: readonly number[] | undefined;
  /** For a permission request: what this phone knows of its answer. */
  readonly answer: Answer | undefined;
  /** For your message: it waits on the phone for the turn to end. */
  readonly held: boolean;
  readonly watch: boolean;
}

function content(row: conversation.Row, id: string, props: RowProps, arriving: boolean) {
  switch (row.kind) {
    case 'user':
      return <UserBubble id={id} row={row} held={props.held} watch={props.watch} />;
    case 'message':
      return <Message id={id} row={row} />;
    case 'thinking':
      return <Thinking id={id} row={row} />;
    case 'steps':
      return <Steps id={id} row={row} open={props.open} />;
    case 'tool':
      return <Tool id={id} row={row} />;
    case 'edit':
      return <Edit id={id} row={row} />;
    case 'permission':
      return <PermissionCard id={id} row={row} answer={props.answer} watch={props.watch} arriving={arriving} />;
    case 'error':
      return <Failure id={id} row={row} />;
    case 'child':
      return <Child id={id} row={row} open={props.open} />;
    case 'note':
      return <Note id={id} row={row} />;
    case 'footer':
      return <Footer id={id} row={row} />;
  }
}

/**
 * One row of the conversation. It is drawn again only when the row itself, or what the screen
 * knows about it, changes: the model keeps a row the same object until it does.
 */
export const ConversationRow = memo(function ConversationRow(props: RowProps) {
  const { row } = props;
  const actions = useRowActions();
  // Asked once, when the row is first drawn: it arrives then, and never again.
  const [arriving] = useState(() => actions.arriving(row.key));
  const id = rowId(row.key);
  return (
    <Frame id={id} depth={row.depth} lines={props.lines} arriving={arriving} turn={row.kind === 'user'}>
      {content(row, id, props, arriving)}
    </Frame>
  );
});
