// Worklets and Reanimated have no native side under Jest: their own mocks stand in, so
// animations finish at once and worklets run as plain functions.
jest.mock('react-native-worklets', () => require('react-native-worklets/src/mock'));
require('react-native-reanimated').setUpTests();
