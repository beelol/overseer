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
  // Overseer: the bold third theme (AC-103). Deep indigo-black led by gradients: violet light from
  // the top left, navy below, an electric violet-to-blue accent. The workbench takes flat colors, so
  // the gradients live in Overseer's own views (gradient below, read through contributed colors).
  overseer: {
    type: 'dark',
    chrome: '#0A0915', bg: '#0E0C1C', raised: '#18152D', raised2: '#1F1B39', hover: '#1C1934', selected: '#2C2456', selectedInactive: '#231E44',
    border: '#241F42', borderStrong: '#383063',
    text: '#EEECFA', muted: '#AAA5CB', faint: '#6F6A94', lineNumber: '#8E89B2', silver: '#CDCAE6',
    accent: '#AC8EFF', accentStrong: '#6D45F0', accentHover: '#5E36E0', onAccent: '#FFFFFF', accentSoft: '#2C2456',
    link: '#BCA9FF', focus: '#8F6BFF',
    green: '#4FD89B', amber: '#F2B24C', red: '#FF7C8C', blue: '#80A9FF', teal: '#4FD6E6', pink: '#FF96D0',
    addedBg: '#4FD89B1F', removedBg: '#FF7C8C1F', addedLine: '#4FD89B33', removedLine: '#FF7C8C33',
    shadow: '#00000066', scrollbar: '#CDCAE626', scrollbarHover: '#CDCAE640',
    syntax: { comment: '#9590B8', keyword: '#C9A6FF', storage: '#C9A6FF', function: '#8FBAFF', type: '#72DCE6', string: '#A8E0A4', number: '#F5BC85', constant: '#F5BC85',
      variable: '#EEECFA', property: '#CDCAE6', tag: '#FF96D0', attribute: '#C9A6FF', operator: '#BAB6D6', punctuation: '#AAA5CB', regexp: '#F5BC85', heading: '#C9A6FF', emphasis: '#EEECFA', invalid: '#FF7C8C' },
    ansi: { black: '#241F42', red: '#FF7C8C', green: '#4FD89B', yellow: '#F0CD85', blue: '#80A9FF', magenta: '#BCA9FF', cyan: '#4FD6E6', white: '#DAD8EC',
      brightBlack: '#6F6A94', brightRed: '#FF98A5', brightGreen: '#75E6B3', brightYellow: '#F6DCA3', brightBlue: '#A2C0FF', brightMagenta: '#D2C4FF', brightCyan: '#7EE4F0', brightWhite: '#FFFFFF' },
    // Gradient stops, each [start, end]. Text is checked against every stop (test/unit/theme-contrast.js).
    gradient: {
      backdrop: ['#171236', '#090B1A'], chrome: ['#141030', '#0A0915'], surface: ['#1A1636', '#110F24'],
      accent: ['#6D45F0', '#2F5FD8'], glow: '#7A4DFF2E',
    },
  },
};

module.exports = { scale, palettes };
