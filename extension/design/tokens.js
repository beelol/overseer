// Overseer design tokens: the single source for the Overseer Dark / Light color themes and the
// CSS custom properties every Overseer webview uses (docs/rfcs/orchestrator-ui.md, AC-54/56).
//
// Palettes are stepped like Geist and derived from three ideas like Linear's themes: a graphite
// base with a faint violet cast, a purple accent, and silver neutrals for icons and rules.
// Webviews never read these hex values directly: they use VS Code theme variables through
// media/tokens.css, so they also look right in any other theme (including high contrast).

const scale = {
  space: { 1: 4, 2: 8, 3: 12, 4: 16, 5: 20, 6: 24, 8: 32, 10: 40 },
  radius: { control: 6, card: 12, pill: 999 },
  font: { xs: 11, sm: 12, md: 13, lg: 14, xl: 16, xxl: 20 },
  weight: { regular: 400, medium: 500, semibold: 600 },
  line: { tight: 1.3, body: 1.6 },
  motion: { fast: 120, base: 160, slow: 200 },
  chat: { column: 720, gutter: 24, gutterNarrow: 16, turnGap: 24, blockGap: 8 },
};

const palettes = {
  dark: {
    type: 'dark',
    // Backgrounds, dimmest (chrome) to raised (cards, bubbles, menus).
    chrome: '#121118', bg: '#17161D', raised: '#211F2A', raised2: '#282633', hover: '#24222E', selected: '#2F2A45', selectedInactive: '#26233A',
    border: '#2A2835', borderStrong: '#38354A',
    text: '#E8E6F0', muted: '#A7A3B9', faint: '#6C687F', lineNumber: '#8A869E', silver: '#C8C6D4',
    accent: '#A48BFF', accentStrong: '#6E4FE6', accentHover: '#5F40D8', onAccent: '#FFFFFF', accentSoft: '#2F2A45',
    link: '#B8A4FF', focus: '#8D72F5',
    green: '#4CC38A', amber: '#E5A33C', red: '#F0707B', blue: '#7AA2F7', teal: '#56C8D8', pink: '#FF9AC1',
    addedBg: '#4CC38A1F', removedBg: '#F0707B1F', addedLine: '#4CC38A33', removedLine: '#F0707B33',
    shadow: '#0000004D', scrollbar: '#C8C6D426', scrollbarHover: '#C8C6D440',
    syntax: { comment: '#8E8AA3', keyword: '#C4A1FF', storage: '#C4A1FF', function: '#8FB8FF', type: '#7FD4D4', string: '#A6D8A2', number: '#F2B880', constant: '#F2B880',
      variable: '#E8E6F0', property: '#C8C6D4', tag: '#FF9AC1', attribute: '#C4A1FF', operator: '#B8B4C8', punctuation: '#A7A3B9', regexp: '#F2B880', heading: '#C4A1FF', emphasis: '#E8E6F0', invalid: '#F0707B' },
    ansi: { black: '#2A2835', red: '#F0707B', green: '#4CC38A', yellow: '#E5C07B', blue: '#7AA2F7', magenta: '#B8A4FF', cyan: '#56C8D8', white: '#D6D4E0',
      brightBlack: '#6C687F', brightRed: '#FF8F98', brightGreen: '#6FDCA6', brightYellow: '#F2D08F', brightBlue: '#9ABBFF', brightMagenta: '#CDBDFF', brightCyan: '#7ADCE8', brightWhite: '#F4F3F8' },
  },
  light: {
    type: 'light',
    chrome: '#F2F1F7', bg: '#FBFAFD', raised: '#FFFFFF', raised2: '#EFEEF5', hover: '#EBEAF2', selected: '#E3DDFB', selectedInactive: '#ECE8FB',
    border: '#E4E2EC', borderStrong: '#D2CFDE',
    text: '#1C1A26', muted: '#5C576F', faint: '#9A96AC', lineNumber: '#6E6A80', silver: '#8D89A0',
    accent: '#6B47E0', accentStrong: '#6B47E0', accentHover: '#5A37CC', onAccent: '#FFFFFF', accentSoft: '#ECE8FB',
    link: '#5B37D0', focus: '#7A5AF0',
    green: '#1A7449', amber: '#915608', red: '#C92F45', blue: '#2F5FCC', teal: '#0B6C76', pink: '#B8175A',
    addedBg: '#1D7F4F1A', removedBg: '#C92F451A', addedLine: '#1D7F4F2E', removedLine: '#C92F452E',
    shadow: '#1C1A2622', scrollbar: '#5C576F26', scrollbarHover: '#5C576F40',
    syntax: { comment: '#6E6A80', keyword: '#7338D8', storage: '#7338D8', function: '#2F5FCC', type: '#0E7384', string: '#2B7A34', number: '#A85300', constant: '#A85300',
      variable: '#1C1A26', property: '#4A4660', tag: '#B8175A', attribute: '#7338D8', operator: '#4A4660', punctuation: '#5C576F', regexp: '#A85300', heading: '#7338D8', emphasis: '#1C1A26', invalid: '#C92F45' },
    ansi: { black: '#1C1A26', red: '#C92F45', green: '#1D7F4F', yellow: '#8A6100', blue: '#2F5FCC', magenta: '#7338D8', cyan: '#0E7384', white: '#9A96AC',
      brightBlack: '#5C576F', brightRed: '#B0243A', brightGreen: '#166B42', brightYellow: '#765300', brightBlue: '#2450B0', brightMagenta: '#5F2CB8', brightCyan: '#0B6470', brightWhite: '#C4C1D1' },
  },
};

module.exports = { scale, palettes };
