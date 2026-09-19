import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./test",
  testMatch: "**/*.spec.mjs",
  fullyParallel: true,
  workers: 2,
  forbidOnly: !!process.env.CI,
  retries: 0,
  use: {
    browserName: "chromium",
    viewport: { width: 1280, height: 900 },
    colorScheme: "dark",
    offline: true,
    trace: "retain-on-failure",
  },
});
