import { defineConfig } from "@playwright/test";
import { join } from "node:path";

export default defineConfig({
  testDir: ".",
  outputDir: join("/tmp", `rilgate-playwright-${process.pid}`),
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  timeout: 60_000,
  globalTimeout: 300_000,
  use: { ignoreHTTPSErrors: true, trace: "retain-on-failure" },
  projects: ["chromium", "firefox", "webkit"].map((browserName) => ({
    name: browserName,
    use: { browserName },
  })),
});
