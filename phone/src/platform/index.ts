/**
 * The platform layer (AC-134): one typed, generic interface per capability, an implementation
 * per platform behind it, and a fake for tests. See README.md in this directory.
 *
 * Screens and shared code import from here. Two things are imported from their own directory:
 * the device's capabilities (`./native`), by the app's root only, and the fakes (`./fake`), by
 * tests only. Shared code therefore never loads a native module, and the app never ships a fake.
 */
export * from './capability';
export * from './capabilities';
export * from './live';
export * from './stores';
export { formatAddress, parseManualAddress, sameAddress } from './addresses';
export { PlatformProvider, useCapabilities, useLive } from './context';
