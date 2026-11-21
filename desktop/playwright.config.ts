import { defineConfig, devices } from "@playwright/test";

// UI tests run against the production build in "preview mode" (simulated devices,
// clearly labelled in the UI). The real Tauri bridge is covered by the Rust tests.
export default defineConfig({
  testDir: "tests/e2e",
  timeout: 45_000,
  fullyParallel: true,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: { baseURL: "http://127.0.0.1:4173", trace: "off", viewport: { width: 1280, height: 820 } },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 820 } } }],
  webServer: { command: "pnpm build && pnpm preview", url: "http://127.0.0.1:4173", reuseExistingServer: !process.env.CI, timeout: 120_000 },
});
