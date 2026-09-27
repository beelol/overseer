// Lint for the phone app. Besides Expo's rules it holds two of the app's own checks:
//
//   1. The platform rule (AC-134): nothing outside src/platform/ may ask which platform it
//      runs on, import a platform's file, or reach a platform library directly.
//      scripts/check-platform-rule.mjs proves the rule with a seeded violation.
//   2. The token rule (AC-131, AC-137): no colour, font size, spacing, radius or duration is
//      written by hand in src/ or app/. They come from src/theme/tokens.generated.ts.
//
// The other packages under phone/ (core/, model/, ...) have their own checks. From here they get
// the platform rule and nothing else: it holds for everything that ends up in the app.

const tsParser = require('@typescript-eslint/parser');
const { defineConfig, globalIgnores } = require('eslint/config');
const expo = require('eslint-config-expo/flat');

const PLATFORM_LAYER = 'src/platform/**';
const GENERATED_TOKENS = 'src/theme/tokens.generated.ts';
// The app's own files: its three directories and the files at the top of phone/.
const OWN = ['app/**', 'src/**', 'scripts/**', '*'];
const CODE = '**/*.{js,jsx,mjs,cjs,ts,tsx}';
const SOURCE = ['**/*.{ts,tsx,js,jsx,mjs,cjs}'];
const SCREENS_AND_SHARED = ['src/**/*.{ts,tsx}', 'app/**/*.{ts,tsx}'];

const USE_A_CAPABILITY =
  'Only src/platform/ may do this. Use a capability from the platform layer (src/platform/README.md).';

// Libraries and React Native modules that sit behind a capability.
const PLATFORM_LIBRARIES = [
  'expo-camera',
  'expo-crypto',
  'expo-device',
  'expo-haptics',
  'expo-local-authentication',
  'expo-network',
  'expo-notifications',
  'expo-secure-store',
  'expo-sqlite',
];
const PLATFORM_EXPORTS_OF_REACT_NATIVE = [
  'Platform',
  'PlatformColor',
  'Appearance',
  'useColorScheme',
  'AppState',
  'AccessibilityInfo',
  'Vibration',
  'BackHandler',
  'ActionSheetIOS',
  'PermissionsAndroid',
  'ToastAndroid',
];

const platformImports = {
  paths: [
    {
      name: 'react-native',
      importNames: PLATFORM_EXPORTS_OF_REACT_NATIVE,
      message: USE_A_CAPABILITY,
    },
    { name: 'expo-modules-core', importNames: ['Platform'], message: USE_A_CAPABILITY },
    ...PLATFORM_LIBRARIES.map((name) => ({ name, message: USE_A_CAPABILITY })),
  ],
  patterns: [
    {
      regex: '\\.(ios|android)(\\.[cm]?[jt]sx?)?$',
      message: `A platform's file is chosen by the bundler, never imported by name. ${USE_A_CAPABILITY}`,
    },
    {
      regex: `^(${PLATFORM_LIBRARIES.join('|')})/`,
      message: USE_A_CAPABILITY,
    },
  ],
};

const platformSyntax = [
  {
    selector: "MemberExpression[object.name='Platform'][property.name='OS']",
    message: `Platform.OS: ${USE_A_CAPABILITY}`,
  },
  {
    selector: "MemberExpression[object.name='Platform'][property.name='select']",
    message: `Platform.select: ${USE_A_CAPABILITY}`,
  },
  {
    selector: "MemberExpression[object.name='Platform'][property.name!=/^(OS|select)$/]",
    message: `Platform: ${USE_A_CAPABILITY}`,
  },
  {
    // ReactNative.Platform, require('react-native').Platform
    selector: "MemberExpression[property.name='Platform']",
    message: `Platform: ${USE_A_CAPABILITY}`,
  },
  {
    selector:
      "VariableDeclarator[init.callee.name='require'] > ObjectPattern > Property[key.name='Platform']",
    message: `Platform: ${USE_A_CAPABILITY}`,
  },
  {
    selector:
      ":matches(CallExpression[callee.name='require'], ImportExpression) > Literal[value=/\\.(ios|android)(\\.[cm]?[jt]sx?)?$/]",
    message: `A platform's file is chosen by the bundler, never imported by name. ${USE_A_CAPABILITY}`,
  },
  {
    selector: ':matches(BinaryExpression, SwitchCase) > Literal[value=/^(ios|android)$/]',
    message: `A comparison with a platform's name is a platform test. ${USE_A_CAPABILITY}`,
  },
];

const USE_A_TOKEN = 'Take it from the theme (useTheme), which is generated from the design tokens.';
const SPACING =
  '(padding|margin)(Top|Bottom|Left|Right|Start|End|Horizontal|Vertical|Block|Inline)?';
const SIZED = `^(fontSize|lineHeight|letterSpacing|gap|rowGap|columnGap|${SPACING}|border\\w*Radius|duration|delay)$`;
const COLOURED = '([cC]olor)$';

const tokenSyntax = [
  {
    selector: 'Literal[value=/^#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/]',
    message: `A colour written by hand. ${USE_A_TOKEN}`,
  },
  {
    selector: 'Literal[value=/^(rgb|rgba|hsl|hsla|hwb)\\(/]',
    message: `A colour written by hand. ${USE_A_TOKEN}`,
  },
  {
    selector: 'TemplateElement[value.raw=/#[0-9a-fA-F]{6}|(rgb|rgba|hsl|hsla)\\(/]',
    message: `A colour written by hand. ${USE_A_TOKEN}`,
  },
  {
    selector: `:matches(Property[key.name=/${COLOURED}/], JSXAttribute[name.name=/${COLOURED}/]) > Literal[value!='transparent']`,
    message: `A colour written by hand. ${USE_A_TOKEN}`,
  },
  {
    selector: `Property[key.name=/${SIZED}/] > Literal[value!=0]`,
    message: `A size, space, radius or duration written by hand. ${USE_A_TOKEN}`,
  },
  {
    selector: `Property[key.name=/${SIZED}/] > UnaryExpression > Literal`,
    message: `A size, space, radius or duration written by hand. ${USE_A_TOKEN}`,
  },
  {
    selector: "Property[key.name='fontWeight'] > Literal",
    message: `A font weight written by hand. ${USE_A_TOKEN}`,
  },
];

/** `patterns`, narrowed to the app's own files. A list inside `files` means "all of these". */
function own(patterns) {
  return OWN.flatMap((place) => patterns.map((pattern) => [place, ...[pattern].flat()]));
}

/** Expo's rules, for the app's own files only. An entry with nothing but `ignores` is global. */
const expoForTheApp = expo.map((entry) =>
  Object.keys(entry).every((key) => key === 'ignores' || key === 'name')
    ? entry
    : { ...entry, files: own(entry.files ?? [CODE]) },
);

module.exports = defineConfig([
  globalIgnores([
    '**/node_modules/**',
    '**/dist/**',
    '**/coverage/**',
    'ios/**',
    'android/**',
    '.expo/**',
    'expo-env.d.ts',
  ]),
  ...expoForTheApp,
  {
    name: 'overseer/typescript-everywhere',
    files: ['**/*.{ts,tsx,mts,cts}'],
    languageOptions: { parser: tsParser },
  },
  {
    name: 'overseer/strict',
    files: own(['**/*.{ts,tsx}']),
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/no-non-null-assertion': 'error',
    },
  },
  {
    name: 'overseer/node-scripts',
    files: ['scripts/**/*.mjs', '*.config.js'],
    languageOptions: { globals: { console: 'readonly', process: 'readonly' } },
  },
  {
    name: 'overseer/jest-setup',
    files: ['jest.setup.js'],
    languageOptions: { globals: { jest: 'readonly' } },
  },
  {
    name: 'overseer/tests',
    files: ['**/__tests__/**/*.{ts,tsx}'],
    rules: {
      // jest.isolateModules loads a module again under other mocks, which only require() can do.
      '@typescript-eslint/no-require-imports': 'off',
    },
  },
  // Flat config replaces a rule's options instead of merging them, so each group of files
  // below names the complete list of what it forbids.
  {
    name: 'overseer/platform-rule',
    files: SOURCE,
    // e2e/ drives the simulators from the Mac and never ends up in the app.
    ignores: [PLATFORM_LAYER, ...SCREENS_AND_SHARED, 'e2e/**'],
    rules: {
      'no-restricted-imports': ['error', platformImports],
      'no-restricted-syntax': ['error', ...platformSyntax],
    },
  },
  {
    name: 'overseer/platform-rule-and-token-rule',
    files: SCREENS_AND_SHARED,
    ignores: [PLATFORM_LAYER, GENERATED_TOKENS],
    rules: {
      'no-restricted-imports': ['error', platformImports],
      'no-restricted-syntax': ['error', ...platformSyntax, ...tokenSyntax],
    },
  },
  {
    name: 'overseer/token-rule-in-the-platform-layer',
    files: [`${PLATFORM_LAYER}/*.{ts,tsx}`],
    rules: {
      'no-restricted-syntax': ['error', ...tokenSyntax],
    },
  },
]);
