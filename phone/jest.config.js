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
  clearMocks: true,
};
