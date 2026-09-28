import { fireEvent, screen } from '@testing-library/react-native';
import * as Clipboard from 'expo-clipboard';
import * as Linking from 'expo-linking';

import { markdown } from '@/model';
import { FAKE_IPHONE } from '@/platform/fake';
import { FOLDED_LINES, Markdown } from '@/screens/conversation/Markdown';
import { createOpenStore, OpenProvider } from '@/screens/conversation/open';
import { idsOf, patience, wordsOf } from '@/screens/conversation/testing';
import { palettes } from '@/theme/tokens.generated';
import { createTestApp } from '@/testing';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('expo-linking', () => ({ openURL: jest.fn(async () => true) }));
jest.mock('expo-clipboard', () => ({ setStringAsync: jest.fn(async () => true) }));

patience();

/** Draws a reply, as the conversation does. */
async function draw(source: string, home = '/Users/bilal') {
  const app = await createTestApp({ launch: FAKE_IPHONE });
  const blocks = markdown.parse(source, { home });
  await app.render(
    <OpenProvider value={createOpenStore()}>
      <Markdown id="md" blocks={blocks} />
    </OpenProvider>,
  );
  return { app, blocks };
}

const words = (testID: string): string => wordsOf(screen.getByTestId(testID));
const styleOf = (element: { props: { style?: unknown } }): Record<string, unknown> => Object.assign({}, ...[element.props.style].flat(4).filter(Boolean));

describe('blocks', () => {
  test('paragraphs and headings', async () => {
    const { blocks } = await draw('# One\n\n## Two\n\n### Three\n\nA paragraph\nof two lines.\n\nAnother.');
    expect(blocks.map((b) => b.type)).toEqual(['heading', 'heading', 'heading', 'paragraph', 'paragraph']);
    const headings = screen.getAllByRole('header');
    expect(headings.map((h) => wordsOf(h))).toEqual(['One', 'Two', 'Three']);
    const sizes = headings.map((h) => Number(styleOf(h)['fontSize']));
    expect(sizes[0]).toBeGreaterThan(Number(sizes[1]));
    expect(styleOf(headings[1] as never)['fontWeight']).toBe('600');
    expect(screen.getByText('A paragraph\nof two lines.')).toBeTruthy();
    expect(screen.getByText('Another.')).toBeTruthy();
  });

  test('lists: bullets, numbers from where they start, nested lists', async () => {
    await draw('- one\n- two\n  - inside\n\n3. three\n4. four');
    expect(screen.getAllByText('•')).toHaveLength(3);
    expect(screen.getByText('3.')).toBeTruthy();
    expect(screen.getByText('4.')).toBeTruthy();
    for (const item of ['one', 'two', 'inside', 'three', 'four']) expect(screen.getByText(item)).toBeTruthy();
  });

  test('task boxes say whether they are done and cannot be pressed', async () => {
    await draw('- [x] written\n- [ ] reviewed');
    const boxes = screen.getAllByRole('checkbox');
    expect(boxes.map((b) => [b.props.accessibilityLabel, b.props.accessibilityState])).toEqual([
      ['Done', { checked: true, disabled: true }],
      ['Not done', { checked: false, disabled: true }],
    ]);
    expect(screen.getByText('written')).toBeTruthy();
    expect(screen.queryByText('•')).toBeNull();
  });

  test('a table scrolls sideways and keeps its columns', async () => {
    await draw('| Case | Before | After |\n| :--- | :---: | ---: |\n| 3 tabs | 3 calls | 1 call |\n| fails | spinner | **error** once |');
    const table = screen.getByTestId('md.0.table');
    expect(table.props.horizontal).toBe(true);
    expect(wordsOf(table)).toBe('CaseBeforeAfter3 tabs3 calls1 callfailsspinnererror once');
    expect(styleOf(screen.getByText('Before'))['textAlign']).toBe('center');
    expect(styleOf(screen.getByText('1 call'))['textAlign']).toBe('right');
    expect(styleOf(screen.getByText('3 tabs'))['textAlign']).toBe('left');
  });

  test('a code block is in a mono font, says its language, and Copy copies all of it', async () => {
    const { app } = await draw('```ts\nconst a = 1;\nconst b = 2;\n```');
    expect(wordsOf(screen.getByTestId('md.0.code'))).toContain('ts');
    const code = screen.getByTestId('md.0.text');
    expect(wordsOf(code)).toBe('const a = 1;\nconst b = 2;');
    expect(styleOf(code)['fontFamily']).toBe('Menlo');
    expect(screen.queryByTestId('md.0.more')).toBeNull();
    await fireEvent.press(screen.getByLabelText('Copy code'));
    expect(Clipboard.setStringAsync).toHaveBeenCalledWith('const a = 1;\nconst b = 2;');
    expect(screen.getByLabelText('Copied')).toBeTruthy();
    expect(app.platform.fakes.haptics.played()).toContain('confirm');
  });

  test('a code block with no language says text', async () => {
    await draw('```\nplain\n```');
    expect(wordsOf(screen.getByTestId('md.0.code'))).toBe('textplain');
  });

  test('a long code block is folded until it is opened; Copy still copies all of it', async () => {
    const lines = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`);
    const { blocks } = await draw('```\n' + lines.join('\n') + '\n```');
    const block = blocks[0] as markdown.CodeBlock;
    expect(block.collapsed).toBe(true);
    expect(words('md.0.text').split('\n')).toEqual(lines.slice(0, FOLDED_LINES));
    expect(words('md.0.more')).toBe(`Show all ${block.lines} lines`);
    await fireEvent.press(screen.getByTestId('md.0.copy'));
    expect(Clipboard.setStringAsync).toHaveBeenCalledWith(lines.join('\n'));
    await fireEvent.press(screen.getByTestId('md.0.more'));
    expect(words('md.0.text').split('\n')).toEqual(lines);
    expect(screen.queryByTestId('md.0.more')).toBeNull();
  });

  test('a quote and a rule', async () => {
    await draw('> quoted words\n> > deeper\n\n---\n\nafter');
    expect(wordsOf(screen.getByTestId('md.0.quote'))).toBe('quoted wordsdeeper');
    expect(styleOf(screen.getByText('quoted words'))['color']).toBe(palettes.light.muted);
    expect(screen.getByTestId('md.1.rule')).toBeTruthy();
    expect(styleOf(screen.getByText('after'))['color']).toBe(palettes.light.text);
  });

  test('raw HTML never becomes a view: its words stay, what a script holds goes', async () => {
    await draw('Hello <b>bold</b> <script>alert(1)</script>world <img src=x onerror=alert(1)>');
    const shown = wordsOf(screen.toJSON());
    expect(shown).toContain('Hello');
    expect(shown).toContain('bold');
    expect(shown).toContain('world');
    expect(shown).not.toContain('alert');
    expect(shown).not.toContain('<');
    expect(idsOf(screen.toJSON())).toEqual([]);
  });
});

describe('inline', () => {
  test('strong, emphasis, strike and code', async () => {
    await draw('Text **strong** and *emphasis* and ~~struck~~ and `code()` here.');
    expect(styleOf(screen.getByText('strong'))['fontWeight']).toBe('600');
    expect(styleOf(screen.getByText('emphasis'))['fontStyle']).toBe('italic');
    expect(styleOf(screen.getByText('struck'))['textDecorationLine']).toBe('line-through');
    const code = screen.getByText('code()');
    expect(styleOf(code)['fontFamily']).toBe('Menlo');
    expect(styleOf(code)['backgroundColor']).toBe(palettes.light.raised2);
    expect(wordsOf(screen.toJSON())).toBe('Text strong and emphasis and struck and code() here.');
  });

  test('a line break breaks the line', async () => {
    await draw('one  \ntwo');
    expect(wordsOf(screen.toJSON())).toBe('one\ntwo');
  });

  test('a link to http or https opens in the phone’s browser', async () => {
    await draw('See the [refresh token spec](https://example.com/spec) and <http://example.org/>.');
    const link = screen.getByRole('link', { name: 'refresh token spec' });
    expect(styleOf(link)['color']).toBe(palettes.light.link);
    await fireEvent.press(link);
    expect(Linking.openURL).toHaveBeenCalledTimes(1);
    expect(Linking.openURL).toHaveBeenLastCalledWith('https://example.com/spec');
    await fireEvent.press(screen.getByRole('link', { name: 'http://example.org/' }));
    expect(Linking.openURL).toHaveBeenLastCalledWith('http://example.org/');
  });

  test('unsafe and other addresses are plain words and open nothing', async () => {
    const { blocks } = await draw('[run](javascript:alert(1)) [data](data:text/html,<b>x</b>) [mail](mailto:a@example.com) [file](file:///etc/passwd) [here](./README.md) [vb](vbscript:msgbox)');
    const links = (blocks[0] as markdown.Paragraph).children.filter((n): n is markdown.Link => n.type === 'link');
    expect(links.length).toBeGreaterThanOrEqual(5);
    expect(links.every((l) => !l.opens)).toBe(true);
    expect(screen.queryAllByRole('link')).toHaveLength(0);
    for (const id of idsOf(screen.toJSON())) {
      const element = screen.getByTestId(id);
      expect(element.props.onPress).toBeUndefined();
      await fireEvent.press(element);
    }
    for (const label of ['run', 'mail', 'here']) await fireEvent.press(screen.getByText(label));
    expect(Linking.openURL).not.toHaveBeenCalled();
    expect(wordsOf(screen.toJSON())).toContain('run');
    expect(wordsOf(screen.toJSON())).not.toContain('javascript:');
  });

  test('a long token shows short and the whole of it on a tap', async () => {
    const path = '/Users/bilal/projects/overseer/phone/src/screens/conversation/rows/PermissionCard.tsx';
    const { blocks } = await draw(`Full path: ${path} and the rest.`);
    const long = (blocks[0] as markdown.Paragraph).children.find((n): n is markdown.Long => n.type === 'long');
    expect(long?.full).toBe(path);
    expect(long?.text.length).toBeLessThan(path.length);
    const token = screen.getByLabelText(path);
    expect(wordsOf(token)).toBe(long?.text);
    expect(token.props.accessibilityState).toEqual({ expanded: false });
    await fireEvent.press(token);
    expect(wordsOf(screen.getByLabelText(path))).toBe(path);
    expect(screen.getByLabelText(path).props.accessibilityState).toEqual({ expanded: true });
    await fireEvent.press(screen.getByLabelText(path));
    expect(wordsOf(screen.getByLabelText(path))).toBe(long?.text);
  });

  test('a long token inside code is shortened too', async () => {
    const token = 'a'.repeat(30) + 'b'.repeat(40);
    await draw(`Run \`${token}\` now.`);
    const short = screen.getByLabelText(token);
    expect(wordsOf(short).length).toBeLessThan(token.length);
    await fireEvent.press(short);
    expect(wordsOf(screen.getByLabelText(token))).toBe(token);
  });

  test('text follows the system’s size up to the largest standard size', async () => {
    await draw('Plain **strong** `code` [link](https://example.com)');
    for (const label of ['strong', 'code', 'link']) expect(screen.getByText(label).props.maxFontSizeMultiplier).toBe(1.6);
  });
});
