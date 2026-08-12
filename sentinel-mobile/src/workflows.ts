// Running a workflow from the phone (card #178) — the "one tap and the gate opens" path.
//
// The whole journey is: tap → the graph runs → its Device Gate asks THIS phone → slide + Face ID →
// the gate opens. The tap can come from inside the app or from a home-screen widget deep-linking
// `olharai://run/<id>`; both land in the same place, because a widget must not become a second
// implementation of "run a workflow".
import { api } from "./session";

/** A graph as the phone lists it. Only the fields a button needs — the editor lives on the web. */
export type WorkflowSummary = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  group: string;
  /** How many nodes; used only to spot an empty graph that would do nothing when tapped. */
  nodes: number;
};

type RawGraph = {
  id: string;
  name?: string;
  description?: string;
  enabled?: boolean;
  group?: string;
  nodes?: unknown[];
};

/** Every workflow in the tenant, newest API shape flattened to what a button needs. */
export async function listWorkflows(): Promise<WorkflowSummary[]> {
  const raw = await api<RawGraph[]>("/workflows");
  return raw.map((g) => ({
    id: g.id,
    name: g.name ?? "Untitled",
    description: g.description ?? "",
    enabled: !!g.enabled,
    group: g.group ?? "",
    nodes: (g.nodes ?? []).length,
  }));
}

/**
 * Fire a workflow. Returns when the engine has ACCEPTED it, not when it finishes — a graph with a
 * Device Gate deliberately ends at the ask, so "finished" would be the wrong thing to wait for.
 */
export async function runWorkflow(id: string): Promise<void> {
  await api<string>(`/workflows/${encodeURIComponent(id)}/run`, {
    method: "POST",
    body: JSON.stringify({ inputs: {} }),
  });
}
