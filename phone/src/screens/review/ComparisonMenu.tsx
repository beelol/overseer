import { useCallback, useState } from 'react';
import { ScrollView, useWindowDimensions, View } from 'react-native';

import { review, text } from '@/model';
import { Empty, Icon, Row, Sheet } from '@/ui';

import { choiceId } from './comparison';
import type { Chosen } from './data';
import { WORDS } from './words';

export interface ComparisonMenuProps {
  /** The start of its test ids: `changes.comparison.menu`. */
  readonly testID: string;
  readonly open: boolean;
  readonly onClose: () => void;
  readonly choices: readonly review.ComparisonChoice[];
  readonly branches: readonly string[];
  readonly onChoose: (chosen: Chosen) => void;
}

/** How much of the screen's height the sheet's rows may take before they scroll. */
const TALLEST = 0.6;

/**
 * The comparisons to choose from, as VS Code's picker lists them. One that is unavailable says
 * why and is not a control. "Other branch…" asks for the branch, then for how to compare with it.
 *
 * It is a `Sheet` of `Row`s and not a `Menu`, because a `Menu` cannot hold an item that cannot
 * be chosen (reported: `MenuItem` needs `disabled` and `selected`).
 */
export function ComparisonMenu({ testID, open, onClose, choices, branches, onChoose }: ComparisonMenuProps) {
  const { height } = useWindowDimensions();
  const [branch, setBranch] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);

  // Closed, it opens on the comparisons again the next time.
  const close = useCallback(() => {
    setBranch(null);
    setPicking(false);
    onClose();
  }, [onClose]);

  const choose = useCallback(
    (chosen: Chosen) => {
      close();
      onChoose(chosen);
    },
    [close, onChoose],
  );

  const kinds = branch === null ? null : review.branchChoices(branch);
  const title = kinds ? kinds.title : picking ? WORDS.changes.branchToCompare : WORDS.changes.compareWith;

  return (
    <Sheet testID={testID} open={open} onClose={close} title={title}>
      <ScrollView style={{ maxHeight: height * TALLEST }} keyboardShouldPersistTaps="handled">
        {kinds
          ? kinds.choices.map((kind, index) => <Row key={kind.mode} testID={`${testID}.kind.${kind.mode}`} label={kind.label} divided={index > 0} right={<View />} onPress={() => choose({ mode: kind.mode, branch: kind.branch })} />)
          : picking
            ? branches.length === 0
              ? <Empty testID={`${testID}.branches.empty`} text={WORDS.changes.noBranches} />
              : branches.map((name, index) => <Row key={name} testID={`${testID}.branch.${name}`} label={name} icon="git-branch" divided={index > 0} onPress={() => setBranch(name)} />)
            : choices.map((choice, index) =>
                choice.mode === 'other' ? (
                  <Row key={choice.key} testID={`${testID}.other`} label={choice.label} icon="git-branch" divided={index > 0} onPress={() => setPicking(true)} />
                ) : choice.available ? (
                  <Row
                    key={choice.key}
                    testID={`${testID}.${choiceId(choice)}`}
                    label={choice.label}
                    icon="git-compare"
                    {...(choice.description ? { value: choice.description } : {})}
                    divided={index > 0}
                    right={choice.selected ? <Icon name="check" size="md" tone="accent" /> : <View />}
                    onPress={() => choose({ mode: choice.mode, branch: choice.branch })}
                  />
                ) : (
                  <Row key={choice.key} testID={`${testID}.${choiceId(choice)}`} label={choice.label} icon="circle-slash" value={text.TEXT.review.unavailable} {...(choice.detail ? { detail: choice.detail } : {})} divided={index > 0} />
                ),
              )}
      </ScrollView>
    </Sheet>
  );
}
