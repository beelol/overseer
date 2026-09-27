// Unit tests run on the Mac with no simulator: shared code and screens against the fakes of
// the platform layer. jest-expo supplies React Native's environment and transforms.
/** @type {import('jest').Config} */
module.exports = {
  preset: 'jest-expo',
  roots: ['<rootDir>/src', '<rootDir>/scripts', '<rootDir>/app'],
  testMatch: ['**/__tests__/**/*.test.{ts,tsx}'],
  moduleNameMapper: {
    '^@/(.*)$': '<rootDir>/src/$1',
  },
  // Only the roots above are searched: the other packages under phone/ (core/, model/, ...)
  // have their own tests, and ios/ and android/ are generated.
  modulePathIgnorePatterns: ['<rootDir>/ios/', '<rootDir>/android/'],
  // The connection library's cryptography (@noble) is published as ES modules only.
  transformIgnorePatterns: [
    'node_modules/(?!((jest-)?react-native|@react-native(-community)?|expo(nent)?|@expo(nent)?/.*|@expo-google-fonts/.*|react-navigation|@react-navigation/.*|@sentry/react-native|native-base|react-native-svg|@noble/.*))',
  ],
  setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],
  clearMocks: true,
};
