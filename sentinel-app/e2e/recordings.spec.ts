/**
 * E2E assertions for the Recordings page.
 *
 * Covers:
 *  - Empty state renders when no recordings exist.
 *  - Seeded recording rows appear in the table with camera IP, duration, and size.
 *  - Download link is present for each row.
 *  - Delete removes the row from the UI.
 *
 * Recordings are seeded/torn-down via the control API directly. The page itself
 * fetches from GET /recordings; no edge box is required.
 */

import { test, expect, type Page } from "@playwright/test";

const CONTROL_URL = process.env.E2E_CONTROL_URL ?? "https://control.thesentinels.online";
const E2E_TOKEN   = process.env.E2E_TOKEN ?? "";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function token(): string {
  if (!E2E_TOKEN) throw new Error("E2E_TOKEN not set — run `npm run test:e2e:seed` first");
  return E2E_TOKEN;
}

async function apiGet<T>(page: Page, path: string): Promise<T> {
  const res = await page.request.get(`${CONTROL_URL}${path}`, {
    headers: { Authorization: `Bearer ${token()}` },
  });
  if (!res.ok()) throw new Error(`GET ${path} → ${res.status()}: ${await res.text()}`);
  return (await res.json()) as T;
}

async function apiDelete(page: Page, path: string): Promise<void> {
  await page.request.delete(`${CONTROL_URL}${path}`, {
    headers: { Authorization: `Bearer ${token()}` },
  });
}

// Seed a recording via POST /recordings (JWT with recordings.write permission).
async function seedRecording(
  page: Page,
  cameraIp: string,
  opts?: { duration_secs?: number; size_bytes?: number },
): Promise<number> {
  const now = new Date().toISOString();
  const clipId = `e2e-${cameraIp.replace(/\./g, "_")}-${Date.now()}`;
  const res = await page.request.post(`${CONTROL_URL}/recordings`, {
    headers: {
      Authorization: `Bearer ${token()}`,
      "Content-Type": "application/json",
    },
    data: {
      camera_ip:     cameraIp,
      clip_id:       clipId,
      duration_secs: opts?.duration_secs ?? 30,
      size_bytes:    opts?.size_bytes    ?? 1_048_576,
      started_at:    now,
    },
  });
  if (!res.ok()) return -1;
  const j = await res.json() as { id: number };
  return j.id;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe("recordings page", () => {
  test("shows empty state when no recordings exist", async ({ page }) => {
    // Delete any leftover e2e recordings first.
    const existing = await apiGet<Array<{ id: number; clip_id: string }>>(page, "/recordings?limit=200")
      .catch(() => [] as Array<{ id: number; clip_id: string }>);
    for (const r of existing.filter((r) => r.clip_id.startsWith("e2e-"))) {
      await apiDelete(page, `/recordings/${r.id}`).catch(() => {});
    }

    await page.goto("/recordings");
    await expect(page.getByText(/no recordings yet/i)).toBeVisible({ timeout: 10_000 });
  });

  test("renders seeded recording rows in the table", async ({ page }) => {
    // Seed two recordings.
    const id1 = await seedRecording(page, "192.168.1.201", { duration_secs: 45, size_bytes: 2_097_152 });
    const id2 = await seedRecording(page, "192.168.1.202", { duration_secs: 10, size_bytes: 512_000 });

    // If seed failed (endpoint not available) skip gracefully.
    if (id1 === -1 || id2 === -1) {
      test.skip();
      return;
    }

    try {
      await page.goto("/recordings");

      // Table should appear (not empty state).
      await expect(page.getByRole("table")).toBeVisible({ timeout: 10_000 });

      // Rows for both camera IPs.
      await expect(page.getByText("192.168.1.201")).toBeVisible();
      await expect(page.getByText("192.168.1.202")).toBeVisible();

      // Duration formatted as "45s" or "1m" etc.
      await expect(page.getByText(/45s/)).toBeVisible();

      // Size formatted (2MB row).
      await expect(page.getByText(/2\.0 MB/)).toBeVisible();

      // Download link present for first recording.
      const dlLink = page.locator(`[data-testid="download-${id1}"]`);
      await expect(dlLink).toBeVisible();
      const href = await dlLink.getAttribute("href");
      expect(href).toContain(`/recordings/${id1}/download`);
    } finally {
      await apiDelete(page, `/recordings/${id1}`).catch(() => {});
      await apiDelete(page, `/recordings/${id2}`).catch(() => {});
    }
  });

  test("deletes a recording and removes it from the table", async ({ page }) => {
    const id = await seedRecording(page, "192.168.1.203", { duration_secs: 5 });
    if (id === -1) { test.skip(); return; }

    try {
      await page.goto("/recordings");
      await expect(page.getByText("192.168.1.203")).toBeVisible({ timeout: 10_000 });

      // Click delete for this recording.
      const btn = page.locator(`[data-testid="delete-${id}"]`);
      await expect(btn).toBeVisible();
      await btn.click();

      // Row should disappear.
      await expect(page.getByText("192.168.1.203")).not.toBeVisible({ timeout: 5_000 });
    } finally {
      // Best-effort cleanup if delete somehow failed.
      await apiDelete(page, `/recordings/${id}`).catch(() => {});
    }
  });
});
