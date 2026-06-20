import { test as setup, expect } from "@playwright/test";
import path from "path";

const SESSION_FILE = path.join(import.meta.dirname, ".auth/session.json");

const EMAIL    = process.env.E2E_EMAIL    ?? "";
const PASSWORD = process.env.E2E_PASSWORD ?? "";

setup("authenticate", async ({ page }) => {
  if (!EMAIL || !PASSWORD) {
    throw new Error("E2E_EMAIL and E2E_PASSWORD must be set in .env.test (or as env vars)");
  }

  await page.goto("/login");
  await page.getByLabel(/email/i).fill(EMAIL);
  await page.getByLabel(/password/i).fill(PASSWORD);
  await page.getByRole("button", { name: /sign in|log in/i }).click();

  // Wait until we've left the login page.
  await expect(page).not.toHaveURL(/\/login/, { timeout: 10_000 });

  // Persist cookies + localStorage so every test project starts authenticated.
  await page.context().storageState({ path: SESSION_FILE });
});
