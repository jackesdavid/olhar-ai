// Run — the phone's one-tap surface for workflows (card #178).
//
// The point is a single deliberate tap that does a real thing in the world: open the garage, arm
// the house, turn the lights off. A workflow that ends in a Device Gate then asks this same phone
// to confirm, so the whole journey is tap → Face ID → the gate opens.
//
// This is also what the home-screen widget drives: a widget button deep-links `olharai://run/<id>`,
// which lands in the same `runWorkflow` this screen calls. One implementation, two entry points —
// a widget must not become a second, subtly different way to run a workflow.
//
// THE ROW IS THE BUTTON. It used to be a card with a full-width "Run" button underneath, which cost
// two lines of height per workflow and made a list of six into a scroll. A button inside a button
// is also a question — which part do I press? — and the answer was never the obvious one.
import { useCallback, useEffect, useState } from "react";
import { ActivityIndicator, Pressable, RefreshControl, ScrollView, StyleSheet, Text, View } from "react-native";
import { listWorkflows, runWorkflow, type WorkflowSummary } from "@/workflows";
import { syncNames, togglePinned, usePinned } from "@/pinned";
import { Danger, EmptyState, Loader, Muted, Screen, Title } from "@/styles/ui";
import { radius, space, typeScale, useTheme } from "@/styles/theme";

export default function Run() {
  const [all, setAll] = useState<WorkflowSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [problem, setProblem] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const pinned = usePinned();
  const t = useTheme();

  const load = useCallback(async () => {
    try {
      const list = await listWorkflows();
      setAll(list);
      // A workflow renamed on the web should not leave a stale label on the home screen — the
      // widget has no way to look a name up for itself.
      syncNames(list.map((w) => ({ id: w.id, name: w.name })));
      setProblem(null);
    } catch (e) {
      setProblem((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  async function fire(w: WorkflowSummary) {
    if (busyId) return;
    setBusyId(w.id);
    setNote(null);
    try {
      await runWorkflow(w.id);
      // Deliberately not "done": a graph with a Device Gate has only just started asking someone.
      setNote(`${w.name} — started.`);
    } catch (e) {
      setNote(`${w.name} — ${(e as Error).message}`);
    } finally {
      setBusyId(null);
    }
  }

  /** Starred first — a favourite is the thing you opened this screen to press. */
  const pinnedIds = pinned.map((p) => p.id);
  const ordered = [...all].sort((a, b) => {
    const rank = (id: string) => (pinnedIds.includes(id) ? 0 : 1);
    return rank(a.id) - rank(b.id) || a.name.localeCompare(b.name);
  });

  /** Dim the rows that are not the one running, so the screen shows what it is doing. */
  function rowOpacity(w: WorkflowSummary, pressed: boolean): number {
    if (busyId && busyId !== w.id) return 0.35;
    return pressed ? 0.7 : 1;
  }

  return (
    <Screen>
      <ScrollView
        contentContainerStyle={styles.page}
        refreshControl={
          <RefreshControl refreshing={loading} onRefresh={load} tintColor={t.mutedForeground} />
        }
      >
        <Title>Run</Title>
        <Muted>Tap to start. Star the ones you want on your home-screen widget.</Muted>

        {loading && all.length === 0 ? <Loader label="Loading workflows…" /> : null}
        {problem ? <Danger>{problem}</Danger> : null}
        {note ? <Muted>{note}</Muted> : null}

        {/* One surface, hairline-separated, so six workflows read as a list rather than six cards
            competing for attention. */}
        {ordered.length > 0 ? (
          <View style={[styles.list, { backgroundColor: t.card, borderColor: t.border }]}>
            {ordered.map((w, i) => {
              const starred = pinnedIds.includes(w.id);
              const running = busyId === w.id;
              return (
                <View
                  key={w.id}
                  style={[
                    styles.row,
                    i > 0 && { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: t.border },
                  ]}
                >
                  <Pressable
                    onPress={() => fire(w)}
                    disabled={busyId !== null}
                    style={({ pressed }) => [styles.tap, { opacity: rowOpacity(w, pressed) }]}
                  >
                    <View style={styles.grow}>
                      <Text style={[typeScale.body, styles.name, { color: t.foreground }]} numberOfLines={1}>
                        {w.name}
                      </Text>
                      {w.description ? (
                        <Text style={[typeScale.caption, { color: t.mutedForeground }]} numberOfLines={1}>
                          {w.description}
                        </Text>
                      ) : null}
                    </View>
                    {/* The chevron says "this whole row does something". A spinner replaces it while
                        the run is starting, in the same place, so nothing shifts. */}
                    {running ? (
                      <ActivityIndicator color={t.mutedForeground} />
                    ) : (
                      <Text style={[typeScale.body, { color: t.mutedForeground }]}>›</Text>
                    )}
                  </Pressable>

                  {/* Outside the run Pressable: nesting it would make the star ambiguous with the
                      row, and starring by accident when you meant to run is the worse mistake. */}
                  <Pressable
                    onPress={() => togglePinned({ id: w.id, name: w.name })}
                    hitSlop={10}
                    style={styles.star}
                  >
                    <Text style={{ fontSize: 18, color: starred ? t.foreground : t.mutedForeground }}>
                      {starred ? "★" : "☆"}
                    </Text>
                  </Pressable>
                </View>
              );
            })}
          </View>
        ) : null}

        {!loading && ordered.length === 0 && !problem ? (
          <EmptyState title="No workflows yet">
            Build one in the web app and it shows up here, ready to run with a tap.
          </EmptyState>
        ) : null}
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  page: { padding: space.lg, gap: space.md, paddingBottom: space.xxl * 2 },
  list: { borderWidth: 1, borderRadius: radius.xl, overflow: "hidden" },
  row: { flexDirection: "row", alignItems: "center" },
  tap: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: space.sm,
    paddingVertical: space.md,
    paddingLeft: space.md,
  },
  grow: { flex: 1, gap: 1 },
  name: { fontWeight: "600" },
  star: { paddingHorizontal: space.md, paddingVertical: space.md },
});
