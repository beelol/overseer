import { defineConfig } from "vitest/config";

export default defineConfig({
  // The model's own settings for its tests. Without this, Vite takes the nearest tsconfig.json
  // that includes a test file, which is the phone app's (it extends expo/tsconfig.base), so the
  // model's tests would fail unless the whole app's node_modules were installed too.
  tsconfig: "tsconfig.test.json",
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
    testTimeout: 60_000,
    hookTimeout: 60_000,
    // The parity and speed tests print what they measured.
    silent: false,
    // Speed is measured: one file at a time, so another file's work is not in the numbers.
    fileParallelism: false,
  },
});
