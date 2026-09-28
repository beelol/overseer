import { defineConfig } from "vitest/config";

export default defineConfig({
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
