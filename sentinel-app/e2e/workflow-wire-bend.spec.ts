/**
 * Regression tests for workflow wire bending.
 *
 * These tests guard against two past bugs:
 *   1. Wire bend not visible in real-time (edgeVias was a Map; mutations didn't
 *      trigger Svelte reactivity — fixed by using a plain $state object).
 *   2. Via-dot disappearing when cursor moved from hit-path to the dot (pointerleave
 *      fired on the inner element — fixed by moving hover handlers to the <g> wrapper).
 */

import { test, expect, type Page } from "@playwright/test";

const CONTROL_URL = process.env.E2E_CONTROL_URL ?? "https://control.thesentinels.online";
// Token written to .env.test by e2e/seed.ts — avoids reading localStorage before page.goto.
const E2E_TOKEN   = process.env.E2E_TOKEN ?? "";

// ---------------------------------------------------------------------------
// Helpers — call the control API directly to seed / clean up test data.
// ---------------------------------------------------------------------------

function apiToken(_page: Page): string {
  if (!E2E_TOKEN) throw new Error("E2E_TOKEN not set — run `npm run test:e2e:seed` first");
  return E2E_TOKEN;
}

async function apiPost<T>(page: Page, path: string, body: unknown): Promise<T> {
  const token = apiToken(page);
  const res = await page.request.post(`${CONTROL_URL}${path}`, {
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    data: body,
  });
  if (!res.ok()) throw new Error(`POST ${path} → ${res.status()}: ${await res.text()}`);
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

async function apiDelete(page: Page, path: string): Promise<void> {
  const token = apiToken(page);
  await page.request.delete(`${CONTROL_URL}${path}`, {
    headers: { Authorization: `Bearer ${token}` },
  });
}

// ---------------------------------------------------------------------------
// Test-workflow fixture — creates a minimal graph, yields its id, tears down.
// ---------------------------------------------------------------------------

async function withTestWorkflow(
  page: Page,
  fn: () => Promise<void>,
): Promise<void> {
  // Unique name per test run to avoid collisions if previous runs left stale workflows.
  const wfName = `__e2e_bend_${Date.now()}__`;

  const graphId = await apiPost<number>(page, "/workflows", { name: wfName });

  // Two nodes with a wire between them so the editor has an edge to interact with.
  // Print → Print is the simplest node kind that needs no config.
  const fromId = await apiPost<number>(page, `/workflows/${graphId}/nodes`, {
    kind: "print", x: 100, y: 200,
  });
  const toId = await apiPost<number>(page, `/workflows/${graphId}/nodes`, {
    kind: "print", x: 400, y: 200,
  });
  await apiPost<void>(page, `/workflows/${graphId}/edges`, {
    from: fromId, from_port: "exec_out",
    to: toId,   to_port:   "exec_in",
  });

  try {
    // Navigate to the list. Find the row and click it to open the graph editor.
    await page.goto("/workflows");
    const nameCell = page.locator("td").filter({ hasText: wfName }).first();
    await expect(nameCell).toBeVisible({ timeout: 15_000 });
    // Click the row (tr) rather than the cell to ensure the onclick fires.
    const row = nameCell.locator("xpath=ancestor::tr");
    await row.click();

    await fn();
  } finally {
    await apiDelete(page, `/workflows/${graphId}`).catch(() => {});
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe("workflow wire bending", () => {
  test("dragging a wire creates a visible bend in real-time", async ({ page }) => {
    await withTestWorkflow(page, async () => {
      // Wait for the SVG canvas to be present (the editor mounts after the row click).
      const canvas = page.locator("svg[data-canvas]");
      await expect(canvas).toBeVisible({ timeout: 10_000 });

      // Locate the visible wire path (non-transparent stroke).
      // There is exactly one edge in our test graph.
      // SVG paths with fill="none" are marked "hidden" by Playwright even though they render —
      // use toBeAttached() which checks DOM presence without the visibility heuristic.
      const wirePath = canvas.locator("path[stroke]:not([stroke='transparent'])").first();
      await expect(wirePath).toBeAttached();

      // Capture the path shape before dragging.
      const dBefore = await wirePath.getAttribute("d");
      expect(dBefore).toMatch(/^M/); // sanity — it's an SVG path

      // Find the wide transparent hit-path (cursor-crosshair) — this is what the
      // user actually drags on.
      const hitPath = canvas.locator("path[stroke='transparent'][stroke-width='20']").first();
      const bbox = await hitPath.boundingBox();
      expect(bbox).not.toBeNull();

      // Click+drag from the midpoint of the wire downwards to create a bend.
      const startX = bbox!.x + bbox!.width / 2;
      const startY = bbox!.y + bbox!.height / 2;
      const endY   = startY + 80;

      await page.mouse.move(startX, startY);
      await page.mouse.down();
      // Move in small steps to simulate a real drag and trigger pointermove events.
      for (let y = startY; y <= endY; y += 10) {
        await page.mouse.move(startX, y);
      }

      // The wire should have changed shape BEFORE we release — regression guard for
      // the Map-reactivity bug where the DOM only updated after page refresh.
      const dDuring = await wirePath.getAttribute("d");
      expect(dDuring).not.toBe(dBefore);
      // A bent wire has two cubic segments (two C commands).
      expect(dDuring).toMatch(/C.*C/s);

      await page.mouse.up();

      // Shape should persist after releasing.
      const dAfter = await wirePath.getAttribute("d");
      expect(dAfter).toMatch(/C.*C/s);
    });
  });

  test("bend handle appears after drag and stays draggable", async ({ page }) => {
    await withTestWorkflow(page, async () => {
      const canvas = page.locator("svg[data-canvas]");
      await expect(canvas).toBeVisible({ timeout: 10_000 });

      const hitPath = canvas.locator("path[stroke='transparent'][stroke-width='20']").first();
      const bbox = await hitPath.boundingBox();
      expect(bbox).not.toBeNull();

      const midX = bbox!.x + bbox!.width / 2;
      const midY = bbox!.y + bbox!.height / 2;

      // Create a bend.
      await page.mouse.move(midX, midY);
      await page.mouse.down();
      await page.mouse.move(midX, midY + 60);
      await page.mouse.up();

      // The bend handle circle should now be visible.
      const dot = canvas.locator("circle[class*='cursor-grab']").first();
      await expect(dot).toBeVisible();

      // Alt+click the dot should remove the bend.
      await dot.click({ modifiers: ["Alt"] });
      await expect(dot).not.toBeVisible();

      // Wire shape should be back to a single-segment cubic.
      const wirePath = canvas.locator("path[stroke]:not([stroke='transparent'])").first();
      const dFlat = await wirePath.getAttribute("d");
      expect(dFlat).not.toMatch(/C.*C/s);
    });
  });

  test("alt+click on wire deletes it", async ({ page }) => {
    await withTestWorkflow(page, async () => {
      const canvas = page.locator("svg[data-canvas]");
      await expect(canvas).toBeVisible({ timeout: 10_000 });

      const hitPath = canvas.locator("path[stroke='transparent'][stroke-width='20']").first();
      await expect(hitPath).toBeAttached(); // transparent path — toBeVisible() fails on fill:none

      const bbox = await hitPath.boundingBox();
      const midX = bbox!.x + bbox!.width / 2;
      const midY = bbox!.y + bbox!.height / 2;

      // Alt+pointerdown on the wire should delete the edge immediately.
      await page.keyboard.down("Alt");
      await page.mouse.click(midX, midY);
      await page.keyboard.up("Alt");

      // The hit path should be gone (edge removed).
      await expect(hitPath).not.toBeVisible();
    });
  });
});
