#!/usr/bin/env node
// Generates the Overseer Dark / Light color themes (themes/*.json) and the webview token sheet
// (media/tokens.css) from design/tokens.js. Run: node extension/design/build-themes.js
// test/unit/theme-contrast.js checks every text pair listed by textPairs() against WCAG AA.
const fs = require('fs');
const path = require('path');
const { scale, palettes } = require('./tokens');

function colors(p) {
  const dark = p.type === 'dark';
  return {
    // Base
    'foreground': p.text, 'descriptionForeground': p.muted, 'disabledForeground': p.faint, 'errorForeground': p.red, 'icon.foreground': p.silver,
    'focusBorder': p.focus, 'contrastBorder': '#00000000', 'selection.background': p.accentSoft, 'widget.border': p.border, 'widget.shadow': p.shadow, 'sash.hoverBorder': p.accent,
    'textLink.foreground': p.link, 'textLink.activeForeground': p.accent, 'textBlockQuote.background': p.raised, 'textBlockQuote.border': p.borderStrong,
    'textCodeBlock.background': p.raised2, 'textPreformat.foreground': p.text, 'textPreformat.background': p.raised2, 'textSeparator.foreground': p.border,
    'progressBar.background': p.accent, 'toolbar.hoverBackground': p.hover, 'toolbar.activeBackground': p.selected,
    // Window chrome: quiet, flat, one surface for the frame.
    'titleBar.activeBackground': p.chrome, 'titleBar.activeForeground': p.muted, 'titleBar.inactiveBackground': p.chrome, 'titleBar.inactiveForeground': p.muted, 'titleBar.border': p.chrome,
    'commandCenter.background': p.raised, 'commandCenter.foreground': p.muted, 'commandCenter.border': p.border, 'commandCenter.activeBackground': p.hover, 'commandCenter.activeForeground': p.text, 'commandCenter.inactiveBorder': p.border,
    'activityBar.background': p.chrome, 'activityBar.foreground': p.text, 'activityBar.inactiveForeground': p.muted, 'activityBar.border': p.chrome, 'activityBar.activeBorder': p.accent,
    'activityBar.activeBackground': p.chrome, 'activityBarBadge.background': p.accentStrong, 'activityBarBadge.foreground': p.onAccent,
    'activityBarTop.foreground': p.text, 'activityBarTop.inactiveForeground': p.muted, 'activityBarTop.activeBorder': p.accent,
    'sideBar.background': p.chrome, 'sideBar.foreground': p.text, 'sideBar.border': p.chrome, 'sideBarTitle.foreground': p.muted,
    'sideBarSectionHeader.background': p.chrome, 'sideBarSectionHeader.foreground': p.muted, 'sideBarSectionHeader.border': p.chrome,
    'statusBar.background': p.chrome, 'statusBar.foreground': p.muted, 'statusBar.border': p.chrome, 'statusBar.noFolderBackground': p.chrome, 'statusBar.noFolderForeground': p.muted,
    'statusBar.debuggingBackground': p.accentStrong, 'statusBar.debuggingForeground': p.onAccent, 'statusBarItem.hoverBackground': p.hover, 'statusBarItem.activeBackground': p.selected,
    'statusBarItem.remoteBackground': p.chrome, 'statusBarItem.remoteForeground': p.muted, 'statusBarItem.prominentBackground': p.accentSoft, 'statusBarItem.prominentForeground': p.text,
    'statusBarItem.errorBackground': p.red, 'statusBarItem.errorForeground': dark ? p.chrome : '#FFFFFF', 'statusBarItem.warningBackground': p.amber, 'statusBarItem.warningForeground': dark ? p.chrome : '#FFFFFF',
    'panel.background': p.bg, 'panel.border': p.border, 'panelTitle.activeForeground': p.text, 'panelTitle.inactiveForeground': p.muted, 'panelTitle.activeBorder': p.accent, 'panelSection.border': p.border, 'panelSectionHeader.background': p.bg,
    // Editor groups and tabs: compact and flat, no separators between tabs.
    'editorGroup.border': p.border, 'editorGroupHeader.tabsBackground': p.chrome, 'editorGroupHeader.tabsBorder': p.chrome, 'editorGroupHeader.noTabsBackground': p.bg, 'editorGroupHeader.border': p.chrome,
    'editorGroup.dropBackground': p.accentSoft, 'editorGroup.emptyBackground': p.bg,
    'tab.activeBackground': p.bg, 'tab.activeForeground': p.text, 'tab.inactiveBackground': p.chrome, 'tab.inactiveForeground': p.muted, 'tab.border': p.chrome, 'tab.activeBorder': p.bg, 'tab.activeBorderTop': p.accent,
    'tab.hoverBackground': p.hover, 'tab.hoverForeground': p.text, 'tab.unfocusedActiveBackground': p.bg, 'tab.unfocusedActiveForeground': p.muted, 'tab.unfocusedInactiveForeground': p.muted, 'tab.unfocusedActiveBorderTop': p.borderStrong,
    'tab.lastPinnedBorder': p.border, 'tab.selectedBackground': p.bg, 'tab.selectedForeground': p.text,
    'breadcrumb.foreground': p.muted, 'breadcrumb.focusForeground': p.text, 'breadcrumb.activeSelectionForeground': p.text, 'breadcrumb.background': p.bg, 'breadcrumbPicker.background': p.raised,
    // Editor
    'editor.background': p.bg, 'editor.foreground': p.text, 'editorLineNumber.foreground': p.lineNumber, 'editorLineNumber.activeForeground': p.text,
    'editorCursor.foreground': p.accent, 'editor.selectionBackground': dark ? '#6E4FE64D' : '#6B47E02E', 'editor.inactiveSelectionBackground': dark ? '#6E4FE626' : '#6B47E01A',
    'editor.selectionHighlightBackground': dark ? '#A48BFF1F' : '#6B47E014', 'editor.wordHighlightBackground': dark ? '#A48BFF1F' : '#6B47E014', 'editor.wordHighlightStrongBackground': dark ? '#A48BFF33' : '#6B47E026',
    'editor.findMatchBackground': dark ? '#E5A33C66' : '#E5A33C66', 'editor.findMatchHighlightBackground': dark ? '#E5A33C33' : '#E5A33C33', 'editor.lineHighlightBackground': dark ? '#FFFFFF08' : '#1C1A2608', 'editor.lineHighlightBorder': '#00000000',
    'editor.hoverHighlightBackground': dark ? '#A48BFF1A' : '#6B47E012', 'editor.rangeHighlightBackground': dark ? '#A48BFF14' : '#6B47E00D',
    'editorIndentGuide.background1': p.border, 'editorIndentGuide.activeBackground1': p.borderStrong, 'editorWhitespace.foreground': p.border, 'editorRuler.foreground': p.border,
    'editorBracketMatch.background': p.accentSoft, 'editorBracketMatch.border': '#00000000',
    'editorWidget.background': p.raised, 'editorWidget.foreground': p.text, 'editorWidget.border': p.border, 'editorSuggestWidget.background': p.raised, 'editorSuggestWidget.border': p.border,
    'editorSuggestWidget.selectedBackground': p.selected, 'editorSuggestWidget.highlightForeground': p.accent, 'editorSuggestWidget.foreground': p.text,
    'editorHoverWidget.background': p.raised, 'editorHoverWidget.border': p.border, 'editorGutter.background': p.bg,
    'editorGutter.addedBackground': p.green, 'editorGutter.modifiedBackground': p.accent, 'editorGutter.deletedBackground': p.red,
    'editorError.foreground': p.red, 'editorWarning.foreground': p.amber, 'editorInfo.foreground': p.blue, 'editorHint.foreground': p.muted,
    'editorOverviewRuler.border': '#00000000', 'editorOverviewRuler.addedForeground': p.green, 'editorOverviewRuler.modifiedForeground': p.accent, 'editorOverviewRuler.deletedForeground': p.red,
    'editorStickyScroll.background': p.bg, 'editorStickyScrollHover.background': p.hover,
    'minimap.background': p.bg, 'minimapSlider.background': p.scrollbar, 'scrollbarSlider.background': p.scrollbar, 'scrollbarSlider.hoverBackground': p.scrollbarHover, 'scrollbarSlider.activeBackground': p.scrollbarHover, 'scrollbar.shadow': '#00000000',
    // Diff
    'diffEditor.insertedTextBackground': p.addedBg, 'diffEditor.removedTextBackground': p.removedBg, 'diffEditor.insertedLineBackground': p.addedLine, 'diffEditor.removedLineBackground': p.removedLine,
    'diffEditor.border': p.border, 'diffEditor.diagonalFill': p.border, 'diffEditorGutter.insertedLineBackground': p.addedLine, 'diffEditorGutter.removedLineBackground': p.removedLine,
    'multiDiffEditor.headerBackground': p.raised, 'multiDiffEditor.border': p.border,
    // Lists and trees
    'list.hoverBackground': p.hover, 'list.hoverForeground': p.text, 'list.activeSelectionBackground': p.selected, 'list.activeSelectionForeground': p.text, 'list.activeSelectionIconForeground': p.text,
    'list.inactiveSelectionBackground': p.selectedInactive, 'list.inactiveSelectionForeground': p.text, 'list.focusBackground': p.selected, 'list.focusForeground': p.text, 'list.focusOutline': p.focus, 'list.inactiveFocusOutline': '#00000000',
    'list.highlightForeground': p.accent, 'list.focusHighlightForeground': p.accent, 'list.dropBackground': p.accentSoft, 'list.errorForeground': p.red, 'list.warningForeground': p.amber,
    'tree.indentGuidesStroke': p.border, 'tree.inactiveIndentGuidesStroke': p.border,
    // Inputs and controls
    'input.background': p.raised, 'input.foreground': p.text, 'input.border': p.border, 'input.placeholderForeground': p.muted,
    'inputOption.activeBackground': p.accentSoft, 'inputOption.activeBorder': p.accent, 'inputOption.activeForeground': p.text,
    'inputValidation.errorBackground': p.raised, 'inputValidation.errorBorder': p.red, 'inputValidation.errorForeground': p.text, 'inputValidation.warningBackground': p.raised, 'inputValidation.warningBorder': p.amber,
    'dropdown.background': p.raised, 'dropdown.foreground': p.text, 'dropdown.border': p.border, 'dropdown.listBackground': p.raised,
    'button.background': p.accentStrong, 'button.foreground': p.onAccent, 'button.hoverBackground': p.accentHover, 'button.border': '#00000000', 'button.separator': dark ? '#FFFFFF33' : '#FFFFFF55',
    'button.secondaryBackground': p.raised2, 'button.secondaryForeground': p.text, 'button.secondaryHoverBackground': p.hover,
    'checkbox.background': p.raised, 'checkbox.border': p.borderStrong, 'checkbox.foreground': p.text, 'checkbox.selectBackground': p.raised, 'checkbox.selectBorder': p.accent,
    'badge.background': p.accentSoft, 'badge.foreground': p.text, 'keybindingLabel.background': p.raised2, 'keybindingLabel.foreground': p.text, 'keybindingLabel.border': p.border, 'keybindingLabel.bottomBorder': p.border,
    // Floating layers
    'quickInput.background': p.raised, 'quickInput.foreground': p.text, 'quickInputTitle.background': p.raised, 'quickInputList.focusBackground': p.selected, 'quickInputList.focusForeground': p.text, 'pickerGroup.foreground': p.accent, 'pickerGroup.border': p.border,
    'menu.background': p.raised, 'menu.foreground': p.text, 'menu.selectionBackground': p.selected, 'menu.selectionForeground': p.text, 'menu.separatorBackground': p.border, 'menu.border': p.border,
    'notifications.background': p.raised, 'notifications.foreground': p.text, 'notifications.border': p.border, 'notificationCenterHeader.background': p.raised2, 'notificationCenterHeader.foreground': p.text,
    'notificationToast.border': p.border, 'notificationLink.foreground': p.link, 'notificationsErrorIcon.foreground': p.red, 'notificationsWarningIcon.foreground': p.amber, 'notificationsInfoIcon.foreground': p.accent,
    'peekView.border': p.accent, 'peekViewEditor.background': p.bg, 'peekViewResult.background': p.raised, 'peekViewTitle.background': p.raised, 'peekViewResult.selectionBackground': p.selected, 'peekViewTitleLabel.foreground': p.text, 'peekViewTitleDescription.foreground': p.muted,
    'welcomePage.tileBackground': p.raised, 'walkThrough.embeddedEditorBackground': p.raised,
    // Terminal
    'terminal.background': p.bg, 'terminal.foreground': p.text, 'terminal.border': p.border, 'terminalCursor.foreground': p.accent, 'terminal.selectionBackground': dark ? '#6E4FE64D' : '#6B47E02E',
    ...Object.fromEntries(Object.entries(p.ansi).map(([k, v]) => ['terminal.ansi' + k[0].toUpperCase() + k.slice(1), v])),
    // Git and status colors
    'gitDecoration.addedResourceForeground': p.green, 'gitDecoration.untrackedResourceForeground': p.green, 'gitDecoration.modifiedResourceForeground': dark ? p.amber : p.amber, 'gitDecoration.deletedResourceForeground': p.red,
    'gitDecoration.renamedResourceForeground': p.teal, 'gitDecoration.ignoredResourceForeground': p.faint, 'gitDecoration.conflictingResourceForeground': p.pink,
    'charts.foreground': p.text, 'charts.lines': p.border, 'charts.red': p.red, 'charts.blue': p.blue, 'charts.yellow': p.amber, 'charts.orange': p.amber, 'charts.green': p.green, 'charts.purple': p.accent,
    'testing.iconPassed': p.green, 'testing.iconFailed': p.red, 'testing.iconQueued': p.amber,
    'debugToolBar.background': p.raised, 'debugIcon.startForeground': p.green,
    'settings.headerForeground': p.text, 'settings.modifiedItemIndicator': p.accent, 'settings.focusedRowBackground': p.hover,
    // Symbol colors double as syntax colors in Overseer's webviews (highlighted code in the chat).
    'symbolIcon.keywordForeground': p.syntax.keyword, 'symbolIcon.functionForeground': p.syntax.function, 'symbolIcon.methodForeground': p.syntax.function, 'symbolIcon.classForeground': p.syntax.type,
    'symbolIcon.typeParameterForeground': p.syntax.type, 'symbolIcon.stringForeground': p.syntax.string, 'symbolIcon.numberForeground': p.syntax.number, 'symbolIcon.constantForeground': p.syntax.constant,
    'symbolIcon.variableForeground': p.syntax.variable, 'symbolIcon.propertyForeground': p.syntax.property, 'symbolIcon.booleanForeground': p.syntax.number, 'symbolIcon.operatorForeground': p.syntax.operator,
    'symbolIcon.keyForeground': p.syntax.attribute, 'symbolIcon.namespaceForeground': p.syntax.type, 'symbolIcon.fieldForeground': p.syntax.property, 'symbolIcon.eventForeground': p.syntax.tag,
    'extensionButton.prominentBackground': p.accentStrong, 'extensionButton.prominentForeground': p.onAccent, 'extensionButton.prominentHoverBackground': p.accentHover,
  };
}

function tokenColors(p) {
  const s = p.syntax;
  const r = (scope, foreground, fontStyle) => ({ scope, settings: fontStyle === undefined ? { foreground } : { foreground, fontStyle } });
  return [
    r(['comment', 'punctuation.definition.comment'], s.comment, 'italic'),
    r(['keyword', 'keyword.control', 'keyword.other', 'storage', 'storage.type', 'storage.modifier'], s.keyword),
    r(['keyword.operator', 'punctuation.accessor'], s.operator),
    r(['entity.name.function', 'support.function', 'meta.function-call entity.name.function'], s.function),
    r(['entity.name.type', 'entity.name.class', 'support.type', 'support.class', 'entity.other.inherited-class', 'meta.type.annotation'], s.type),
    r(['string', 'string.quoted', 'string.template', 'punctuation.definition.string'], s.string),
    r(['constant.numeric', 'constant.language', 'constant.character', 'support.constant', 'variable.other.constant'], s.number),
    r(['string.regexp'], s.regexp),
    r(['variable', 'variable.other.readwrite', 'meta.definition.variable'], s.variable),
    r(['variable.other.property', 'variable.other.object.property', 'support.variable.property', 'meta.object-literal.key'], s.property),
    r(['variable.parameter'], s.variable, 'italic'),
    r(['entity.name.tag', 'meta.tag.sgml'], s.tag),
    r(['entity.other.attribute-name'], s.attribute),
    r(['punctuation', 'meta.brace', 'punctuation.definition.tag'], s.punctuation),
    r(['markup.heading', 'entity.name.section'], s.heading, 'bold'),
    r(['markup.bold'], s.emphasis, 'bold'), r(['markup.italic'], s.emphasis, 'italic'),
    r(['markup.inline.raw', 'markup.fenced_code'], s.string), r(['markup.underline.link'], p.link),
    r(['markup.inserted'], p.green), r(['markup.deleted'], p.red), r(['markup.changed'], p.amber),
    r(['invalid'], s.invalid),
  ];
}

/** Every text color and the background it is read on (alpha colors are composited). */
function textPairs(p, c) {
  const pairs = [];
  const add = (fg, bg, what) => pairs.push({ fg, bg, what });
  const bgs = { chrome: p.chrome, editor: p.bg, raised: p.raised, raised2: p.raised2, hover: p.hover, selected: p.selected };
  for (const [name, bg] of Object.entries(bgs)) { add(p.text, bg, `text on ${name}`); add(p.muted, bg, `muted text on ${name}`); }
  add(p.link, p.bg, 'link on editor'); add(p.link, p.raised, 'link on raised'); add(p.accent, p.bg, 'accent text on editor'); add(p.accent, p.chrome, 'accent text on chrome');
  add(c['button.foreground'], c['button.background'], 'primary button'); add(c['button.foreground'], c['button.hoverBackground'], 'primary button hover');
  add(c['button.secondaryForeground'], c['button.secondaryBackground'], 'secondary button'); add(c['badge.foreground'], c['badge.background'], 'badge');
  add(c['activityBarBadge.foreground'], c['activityBarBadge.background'], 'activity badge');
  add(c['tab.activeForeground'], c['tab.activeBackground'], 'active tab'); add(c['tab.inactiveForeground'], c['tab.inactiveBackground'], 'inactive tab');
  add(c['statusBar.foreground'], c['statusBar.background'], 'status bar'); add(c['titleBar.activeForeground'], c['titleBar.activeBackground'], 'title bar');
  add(c['statusBarItem.errorForeground'], c['statusBarItem.errorBackground'], 'status bar error'); add(c['statusBarItem.warningForeground'], c['statusBarItem.warningBackground'], 'status bar warning');
  add(c['input.foreground'], c['input.background'], 'input'); add(c['input.placeholderForeground'], c['input.background'], 'input placeholder');
  add(c['list.activeSelectionForeground'], c['list.activeSelectionBackground'], 'list selection'); add(c['list.highlightForeground'], c['list.hoverBackground'], 'list match');
  add(c['editorLineNumber.foreground'], c['editor.background'], 'line numbers'); add(c['editorLineNumber.activeForeground'], c['editor.background'], 'active line number');
  add(c['terminal.foreground'], c['terminal.background'], 'terminal text');
  for (const [k, v] of Object.entries(p.ansi)) {
    // By convention ANSI black is a background color on dark terminals and white on light ones.
    if (p.type === 'dark' && /black/i.test(k)) continue;
    if (p.type === 'light' && /white/i.test(k)) continue;
    add(v, p.bg, `terminal ${k}`);
  }
  for (const [k, v] of Object.entries(p.syntax)) add(v, p.bg, `syntax ${k}`);
  for (const k of ['green', 'amber', 'red', 'blue', 'teal', 'pink']) { add(p[k], p.bg, `${k} on editor`); add(p[k], p.chrome, `${k} on chrome`); }
  add(p.text, blend(p.addedLine, p.bg), 'text on added line'); add(p.text, blend(p.removedLine, p.bg), 'text on removed line');
  return pairs;
}

function blend(fg, bg) {
  const h = x => x.replace('#', '');
  const f = h(fg), b = h(bg);
  if (f.length !== 8) return fg;
  const a = parseInt(f.slice(6, 8), 16) / 255;
  const ch = i => Math.round(parseInt(f.slice(i, i + 2), 16) * a + parseInt(b.slice(i, i + 2), 16) * (1 - a));
  return '#' + [0, 2, 4].map(i => ch(i).toString(16).padStart(2, '0')).join('').toUpperCase();
}

function contrast(fg, bg) {
  const lum = hex => {
    const [r, g, b] = [0, 2, 4].map(i => parseInt(hex.replace('#', '').slice(i, i + 2), 16) / 255).map(v => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const f = blend(fg, bg), l1 = lum(f), l2 = lum(bg);
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
}

function theme(name, p) {
  return { $schema: 'vscode://schemas/color-theme', name, type: p.type, semanticHighlighting: true, colors: colors(p), tokenColors: tokenColors(p),
    semanticTokenColors: { parameter: { foreground: p.syntax.variable, italic: true }, property: p.syntax.property, 'variable.readonly': p.syntax.constant, enumMember: p.syntax.constant, namespace: p.syntax.type, type: p.syntax.type, class: p.syntax.type, interface: p.syntax.type, function: p.syntax.function, method: p.syntax.function } };
}

/** CSS variables for webviews: scale values, plus semantic colors that map to VS Code theme variables. */
function tokensCss() {
  const px = (prefix, obj) => Object.entries(obj).map(([k, v]) => `  --ov-${prefix}-${k}: ${v}px;`).join('\n');
  return `/* Generated by extension/design/build-themes.js from design/tokens.js. Do not edit by hand.
   Overseer webviews use these tokens only. Colors map to VS Code theme variables, so views follow
   whatever theme is active (Overseer Dark/Light set those variables to the Overseer palette). */
:root {
${px('space', scale.space)}
  --ov-radius-control: ${scale.radius.control}px;
  --ov-radius-card: ${scale.radius.card}px;
  --ov-radius-pill: ${scale.radius.pill}px;
${px('font', scale.font)}
  --ov-weight-regular: ${scale.weight.regular};
  --ov-weight-medium: ${scale.weight.medium};
  --ov-weight-semibold: ${scale.weight.semibold};
  --ov-line-tight: ${scale.line.tight};
  --ov-line-body: ${scale.line.body};
  --ov-motion-fast: ${scale.motion.fast}ms;
  --ov-motion-base: ${scale.motion.base}ms;
  --ov-motion-slow: ${scale.motion.slow}ms;
  --ov-ease: cubic-bezier(.2, .8, .2, 1);
  --ov-chat-column: ${scale.chat.column}px;
  --ov-chat-gutter: ${scale.chat.gutter}px;
  --ov-chat-gutter-narrow: ${scale.chat.gutterNarrow}px;
  --ov-chat-turn-gap: ${scale.chat.turnGap}px;
  --ov-chat-block-gap: ${scale.chat.blockGap}px;

  --ov-font: var(--vscode-font-family);
  --ov-mono: var(--vscode-editor-font-family);
  --ov-bg: var(--vscode-editor-background);
  --ov-chrome: var(--vscode-sideBar-background, var(--vscode-editor-background));
  --ov-raised: var(--vscode-editorWidget-background, var(--vscode-editor-background));
  --ov-raised-2: var(--vscode-textCodeBlock-background, var(--vscode-editorWidget-background));
  --ov-hover: var(--vscode-list-hoverBackground);
  --ov-selected: var(--vscode-list-inactiveSelectionBackground, var(--vscode-list-hoverBackground));
  --ov-selected-strong: var(--vscode-list-activeSelectionBackground);
  --ov-selected-fg: var(--vscode-list-activeSelectionForeground, var(--vscode-foreground));
  --ov-border: var(--vscode-widget-border, var(--vscode-panel-border, transparent));
  --ov-border-strong: var(--vscode-input-border, var(--vscode-panel-border, transparent));
  --ov-text: var(--vscode-foreground);
  --ov-muted: var(--vscode-descriptionForeground);
  --ov-faint: var(--vscode-disabledForeground, var(--vscode-descriptionForeground));
  --ov-icon: var(--vscode-icon-foreground, var(--vscode-foreground));
  --ov-accent: var(--vscode-textLink-foreground);
  --ov-accent-bg: var(--vscode-button-background);
  --ov-accent-bg-hover: var(--vscode-button-hoverBackground);
  --ov-accent-fg: var(--vscode-button-foreground);
  --ov-accent-soft: var(--vscode-inputOption-activeBackground, var(--vscode-list-inactiveSelectionBackground));
  --ov-focus: var(--vscode-focusBorder);
  --ov-link: var(--vscode-textLink-foreground);
  --ov-code-bg: var(--vscode-textCodeBlock-background, var(--vscode-editorWidget-background));
  --ov-input-bg: var(--vscode-input-background);
  --ov-input-fg: var(--vscode-input-foreground);
  --ov-running: var(--vscode-progressBar-background, var(--vscode-charts-blue));
  --ov-waiting: var(--vscode-charts-orange, var(--vscode-editorWarning-foreground));
  --ov-done: var(--vscode-testing-iconPassed, var(--vscode-charts-green));
  --ov-failed: var(--vscode-errorForeground);
  --ov-added: var(--vscode-gitDecoration-addedResourceForeground, var(--vscode-charts-green));
  --ov-removed: var(--vscode-gitDecoration-deletedResourceForeground, var(--vscode-errorForeground));
  --ov-modified: var(--vscode-gitDecoration-modifiedResourceForeground, var(--vscode-charts-yellow));
  --ov-shadow: var(--vscode-widget-shadow, transparent);
  --ov-hc-border: var(--vscode-contrastBorder, transparent);
  --ov-hc-active: var(--vscode-contrastActiveBorder, transparent);
}
`;
}

if (require.main === module) {
  const root = path.join(__dirname, '..');
  fs.mkdirSync(path.join(root, 'themes'), { recursive: true });
  fs.writeFileSync(path.join(root, 'themes/overseer-dark-color-theme.json'), JSON.stringify(theme('Overseer Dark', palettes.dark), null, 2) + '\n');
  fs.writeFileSync(path.join(root, 'themes/overseer-light-color-theme.json'), JSON.stringify(theme('Overseer Light', palettes.light), null, 2) + '\n');
  fs.writeFileSync(path.join(root, 'media/tokens.css'), tokensCss());
  console.log('wrote themes/overseer-dark-color-theme.json, themes/overseer-light-color-theme.json, media/tokens.css');
}

module.exports = { colors, textPairs, contrast, blend, theme, tokensCss };
