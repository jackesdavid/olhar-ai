/**
 * Regression tests for exec fan-in (card #90).
 *
 * The engine has always accepted converging exec paths — `Graph::add_edge` exempts
 * `exec_in` from the single-fed rule ("exec_in accepts fan-in ... e.g. both branches of
 * an If merging downstream"). The editor made it unreachable: `clickInPort` treated a
 * click on an already-wired input as "unwire" and returned before it could add a second
 * wire. These tests guard:
 *   1. A pending output + a wired exec_in now ADDS a converging wire (the fix).
 *   2. The same pin pair twice is still rejected (fan-in newly makes this reachable).
 *   3. Clicking a wired exec_in with nothing pending still unwires (preserved behaviour).
 */

import { test, expect, type Page } from "@playwright/test";

const CONTROL_URL = process.env.E2E_CONTROL_URL ?? "https://control.thesentinels.online";
// Token written to .env.test by e2e/seed.ts — avoids reading localStorage before page.goto.
const E2E_TOKEN = process.env.E2E_TOKEN ?? "";

// ---------------------------------------------------------------------------
// Helpers — call the control API directly to seed / clean up test data.
// ---------------------------------------------------------------------------

function apiToken(): string {
  if (!E2E_TOKEN) throw new Error("E2E_TOKEN not set — run `npm run test:e2e:seed` first");
  return E2E_TOKEN;
}

async function apiPost<T>(page: Page, path: string, body: unknown): Promise<T> {
  const res = await page.request.post(`${CONTROL_URL}${path}`, {
    headers: { Authorization: `Bearer ${apiToken()}`, "Content-Type": "application/json" },
    data: body,
  });
  if (!res.ok()) throw new Error(`POST ${path} → ${res.status()}: ${await res.text()}`);
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

async function apiDelete(page: Page, path: string): Promise<void> {
  await page.request.delete(`${CONTROL_URL}${path}`, {
    headers: { Authorization: `Bearer ${apiToken()}` },
  });
}

// ---------------------------------------------------------------------------
// Fixture — three Print nodes, with A already wired into C's exec_in.
//
//   A ──exec_out──▶ exec_in ┐
//                           ├─ C      (B is the converging path the test wires)
//   B                       ┘
// ---------------------------------------------------------------------------

const A = { x: 100, y: 100 };
const B = { x: 100, y: 320 };
const C = { x: 460, y: 200 };

/** The node <g> is keyed only by its canvas transform — and we choose x/y when seeding. */
function nodeAt(page: Page, pos: { x: number; y: number }) {
  return page.locator(`svg[data-canvas] g[transform="translate(${pos.x} ${pos.y})"]`);
}

// Exec pins render before data pins inside each node's <g>, so for a Print node
// .nth(0) is exec_in (left, cx=0) and .nth(1) is exec_out (right, cx=nodeW).
function execIn(page: Page, pos: { x: number; y: number }) {
  return nodeAt(page, pos).locator("circle.wf-port").nth(0);
}
function execOut(page: Page, pos: { x: number; y: number }) {
  return nodeAt(page, pos).locator("circle.wf-port").nth(1);
}

/** One transparent hit-path is rendered per edge — the easiest way to count wires. */
function wires(page: Page) {
  return page.locator("svg[data-canvas] path[stroke='transparent'][stroke-width='20']");
}

async function withFanInGraph(page: Page, fn: () => Promise<void>): Promise<void> {
  const wfName = `__e2e_fanin_${Date.now()}__`;
  // Graph ids are UUIDs (migration 0033); node ids stay per-graph ints.
  const graphId = await apiPost<string>(page, "/workflows", { name: wfName });

  // Print → Print is the simplest node kind that needs no config.
  const a = await apiPost<number>(page, `/workflows/${graphId}/nodes`, { kind: "print", x: A.x, y: A.y });
  await apiPost<number>(page, `/workflows/${graphId}/nodes`, { kind: "print", x: B.x, y: B.y });
  const c = await apiPost<number>(page, `/workflows/${graphId}/nodes`, { kind: "print", x: C.x, y: C.y });
  await apiPost<void>(page, `/workflows/${graphId}/edges`, {
    from: a, from_port: "exec_out", to: c, to_port: "exec_in",
  });

  try {
    await page.goto("/workflows");
    // The list renders each graph as a button (not a table row).
    const row = page.locator("button").filter({ hasText: wfName }).first();
    await expect(row).toBeVisible({ timeout: 15_000 });
    await row.click();

    await expect(page.locator("svg[data-canvas]")).toBeVisible({ timeout: 10_000 });
    await fn();
  } finally {
    await apiDelete(page, `/workflows/${graphId}`).catch(() => {});
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe("workflow exec fan-in", () => {
  test("a pending output + a wired exec_in adds a converging wire", async ({ page }) => {
    await withFanInGraph(page, async () => {
      await expect(wires(page)).toHaveCount(1);

      // Pick B's exec_out, then click C's exec_in — which A already feeds.
      await execOut(page, B).click();
      await execIn(page, C).click();

      // Before the fix this REMOVED A's wire (count 0). Now both paths converge.
      await expect(wires(page)).toHaveCount(2);

      // And it survives a reload, so the server accepted the second edge.
      await page.reload();
      await expect(page.locator("svg[data-canvas]")).toBeVisible({ timeout: 10_000 });
      await expect(wires(page)).toHaveCount(2);
    });
  });

  test("re-wiring the same pins is rejected, not stacked", async ({ page }) => {
    await withFanInGraph(page, async () => {
      await expect(wires(page)).toHaveCount(1);

      // A → C already exists; wiring the identical pin pair must not duplicate it.
      await execOut(page, A).click();
      await execIn(page, C).click();

      await expect(wires(page)).toHaveCount(1);
    });
  });

  test("clicking a wired exec_in with nothing pending still unwires", async ({ page }) => {
    await withFanInGraph(page, async () => {
      await expect(wires(page)).toHaveCount(1);

      // No output picked → the click keeps its old meaning: remove the wire.
      await execIn(page, C).click();

      await expect(wires(page)).toHaveCount(0);
    });
  });
});
