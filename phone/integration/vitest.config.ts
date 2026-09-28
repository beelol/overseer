import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    testTimeout: 120_000,
    hookTimeout: 60_000,
    // Each file starts its own daemons on their own ports; files may run side by side.
    fileParallelism: true,
  },
});
