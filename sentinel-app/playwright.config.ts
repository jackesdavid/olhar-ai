import { defineConfig, devices } from "@playwright/test";
import { config as dotenv } from "dotenv";

// Load .env.test for local runs; CI injects env vars directly.
dotenv({ path: ".env.test" });

const BASE_URL = process.env.E2E_BASE_URL ?? "https://app.thesentinels.online";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  retries: 1,
  reporter: [["list"], ["html", { open: "never" }]],

  use: {
    baseURL: BASE_URL,
    headless: true,
    // Reuse the auth session saved by global.setup.ts across all tests.
    storageState: "e2e/.auth/session.json",
  },

  projects: [
    // Auth setup runs first, saves session.json used by all test projects.
    {
      name: "setup",
      testMatch: /global\.setup\.ts/,
      use: { storageState: undefined },
    },
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      dependencies: ["setup"],
    },
  ],
});
