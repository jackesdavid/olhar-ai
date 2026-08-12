// Run — the phone's one-tap surface for workflows (card #178).
//
// The point is a single deliberate tap that does a real thing in the world: open the garage, arm
// the house, turn the lights off. A workflow that ends in a Device Gate then asks this same phone
// to confirm, so the whole journey is tap → Face ID → the gate opens.
//
// This is also what the home-screen widget drives: a widget button deep-links `olharai://run/<id>`,
// which lands in the same `runWorkflow` this screen calls. One implementation, two entry points —
// a widget must not become a second, subtly different way to run a workflow.
import { useCallback, useEffect, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { listWorkflows, runWorkflow, type WorkflowSummary } from "@/workflows";
import { togglePinned, usePinned } from "@/pinned";
import { Body, Card, Danger, Heading, Muted, Screen, Strong, Title } from "@/styles/ui";
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
      setAll(await listWorkflows());
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
  const ordered = [...all].sort((a, b) => {
    const rank = (id: string) => (pinned.includes(id) ? 0 : 1);
    return rank(a.id) - rank(b.id) || a.name.localeCompare(b.name);
  });

  function runLabel(w: WorkflowSummary): string {
    return busyId === w.id ? "Starting…" : "Run";
  }
  function runOpacity(w: WorkflowSummary, pressed: boolean): number {
    if (busyId && busyId !== w.id) return 0.4;
    return pressed ? 0.8 : 1;
  }

  return (
    <Screen>
      <ScrollView
        contentContainerStyle={{ padding: space.xl, gap: space.md, paddingBottom: space.xxl * 2 }}
        refreshControl={
          <RefreshControl refreshing={loading} onRefresh={load} tintColor={t.mutedForeground} />
        }
      >
        <Title>Run</Title>
        <Muted>One tap starts a workflow. Star the ones you want on your home-screen widget.</Muted>

        {loading && all.length === 0 ? <ActivityIndicator color={t.mutedForeground} /> : null}
        {problem ? <Danger>{problem}</Danger> : null}
        {note ? <Muted>{note}</Muted> : null}

        {ordered.map((w) => (
          <Card key={w.id}>
            <View style={styles.row}>
              <View style={styles.grow}>
                <Strong>{w.name}</Strong>
                {w.description ? <Muted>{w.description}</Muted> : null}
              </View>
              <Pressable onPress={() => togglePinned(w.id)} hitSlop={12} style={styles.star}>
                <Text style={[typeScale.title, { color: t.foreground }]}>
                  {pinned.includes(w.id) ? "★" : "☆"}
                </Text>
              </Pressable>
            </View>
            <Pressable
              onPress={() => fire(w)}
              disabled={busyId !== null}
              style={({ pressed }) => [
                styles.run,
                {
                  backgroundColor: t.foreground,
                  borderRadius: radius.lg,
                  opacity: runOpacity(w, pressed),
                },
              ]}
            >
              <Text style={[typeScale.body, styles.runText, { color: t.background }]}>
                {runLabel(w)}
              </Text>
            </Pressable>
          </Card>
        ))}

        {!loading && ordered.length === 0 && !problem ? (
          <Card>
            <Heading>No workflows yet</Heading>
            <Body>Build one in the web app and it shows up here, ready to run with a tap.</Body>
          </Card>
        ) : null}
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: "row", alignItems: "flex-start", gap: space.sm },
  grow: { flex: 1, gap: 2 },
  star: { padding: space.sm },
  run: { alignItems: "center", paddingVertical: space.md },
  runText: { fontWeight: "600" },
});
