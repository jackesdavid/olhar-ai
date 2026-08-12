// Home.
//
// It used to be a diagnostics panel: is this phone registered, send a test push, here is a log of
// notifications that arrived. Useful once, on the day you set the phone up, and never again — while
// the thing a person actually opens this app for is "does anything need me right now?".
//
// So the order is urgency, not setup: an approval waiting on you, then what you can start with one
// tap, then whether delivery is healthy. There is one focal point — the top card — and on a quiet
// day the screen says so with the mark, rather than showing an empty log and letting you wonder.
import { useCallback, useEffect, useState } from "react";
import { AppState, RefreshControl, ScrollView, StyleSheet, Text, View } from "react-native";
import * as Notifications from "expo-notifications";
import { useRouter } from "expo-router";
import { registerThisDevice } from "@/push";
import { formatLeft, listPending, secondsLeft, type PendingRequest } from "@/deviceGate";
import { listWorkflows, runWorkflow, type WorkflowSummary } from "@/workflows";
import { syncNames, usePinned } from "@/pinned";
import { ago, loadStats, type Stats } from "@/stats";
import { BarChart, Stat } from "@/styles/Chart";
import {
  Card,
  EmptyState,
  Loader,
  Muted,
  PressableCard,
  Screen,
  Strong,
  Title,
} from "@/styles/ui";
import { space, typeScale, useTheme } from "@/styles/theme";

export default function Home() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [booting, setBooting] = useState(true);
  const [pending, setPending] = useState<PendingRequest[]>([]);
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([]);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const pinned = usePinned();
  const router = useRouter();
  const t = useTheme();

  // Registration still happens here — it is the screen that always loads — but its RESULT lives in
  // Settings now. A dashboard should not spend its best space on a setup detail.
  useEffect(() => {
    registerThisDevice().finally(() => setBooting(false));
  }, []);

  // The countdown ticks locally; the server owns the deadline.
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  const refresh = useCallback(async () => {
    const [reqs, flows, agg] = await Promise.all([
      listPending().catch(() => [] as PendingRequest[]),
      listWorkflows().catch(() => [] as WorkflowSummary[]),
      loadStats().catch(() => null),
    ]);
    setPending(reqs);
    setWorkflows(flows);
    if (agg) setStats(agg);
    syncNames(flows.map((w) => ({ id: w.id, name: w.name })));
  }, []);

  useEffect(() => {
    refresh();
    const poll = setInterval(refresh, 20_000);
    const app = AppState.addEventListener("change", (s) => s === "active" && refresh());
    // A push arriving while the app is open should update the list under the person's thumb.
    const sub = Notifications.addNotificationReceivedListener(() => refresh());
    return () => {
      clearInterval(poll);
      app.remove();
      sub.remove();
    };
  }, [refresh]);

  /** Starred first — the buttons someone put there on purpose lead. */
  const quick = [...workflows]
    .sort((a, b) => {
      const rank = (id: string) => (pinned.some((p) => p.id === id) ? 0 : 1);
      return rank(a.id) - rank(b.id) || a.name.localeCompare(b.name);
    })
    .slice(0, 3);

  async function fire(w: WorkflowSummary) {
    if (runningId) return;
    setRunningId(w.id);
    setNote(null);
    try {
      await runWorkflow(w.id);
      setNote(`${w.name} — started.`);
      await refresh();
    } catch (e) {
      setNote((e as Error).message);
    } finally {
      setRunningId(null);
    }
  }

  return (
    <Screen>
      <ScrollView
        contentContainerStyle={styles.page}
        refreshControl={
          <RefreshControl refreshing={false} onRefresh={refresh} tintColor={t.mutedForeground} />
        }
      >
        {booting ? (
          <Loader label="Waking up…" />
        ) : (
          <>
            {/* Waiting on you — the focal point, and the only thing here that can be urgent. */}
            {pending.map((p) => (
              <PressableCard
                key={p.id}
                onPress={() => router.push({ pathname: "/approve", params: { request_id: p.id } })}
                style={{ borderColor: t.foreground }}
              >
                <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                  WAITING ON YOU
                </Text>
                <Title>{p.prompt}</Title>
                <View style={styles.metaRow}>
                  <Muted>{p.flow_name}</Muted>
                  <Text style={[typeScale.body, styles.mono, { color: t.foreground }]}>
                    {formatLeft(secondsLeft(p.expires_at, now))}
                  </Text>
                </View>
              </PressableCard>
            ))}

            {pending.length === 0 ? (
              <EmptyState title="All quiet">
                Nothing needs you right now. When an automation asks for approval it appears here,
                and your phone buzzes.
              </EmptyState>
            ) : null}

            {/* One tap, the three most likely. The full list is the Run tab. */}
            {quick.length > 0 ? (
              <View style={styles.section}>
                <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                  QUICK RUN
                </Text>
                {quick.map((w) => (
                  <PressableCard key={w.id} onPress={() => fire(w)} disabled={runningId !== null}>
                    <View style={styles.metaRow}>
                      <Strong>{w.name}</Strong>
                      <Muted>{runningId === w.id ? "Starting…" : "Run"}</Muted>
                    </View>
                  </PressableCard>
                ))}
              </View>
            ) : null}

            {note ? <Muted>{note}</Muted> : null}

            {/* The week, in numbers and one chart. Height carries the message; nothing is coloured
                to mean something, because the ink theme has one colour and it is enough. */}
            {stats ? (
              <View style={styles.section}>
                <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                  LAST 7 DAYS
                </Text>
                <Card>
                  <View style={styles.stats}>
                    <Stat value={stats.today} label="today" />
                    <Stat value={stats.week} label="this week" />
                    <Stat value={stats.clips} label="clips" />
                    <Stat value={stats.cameras} label="cameras" />
                  </View>
                  <BarChart bars={stats.perDay} peak={stats.peak} />
                </Card>
              </View>
            ) : null}

            {/* What actually happened, most recent first — the dashboard's proof of life. */}
            {stats && stats.recent.length > 0 ? (
              <View style={styles.section}>
                <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                  ACTIVITY
                </Text>
                <Card>
                  {stats.recent.slice(0, 8).map((e, i) => (
                    <View
                      key={e.id}
                      style={[
                        styles.event,
                        i > 0 && { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: t.border },
                      ]}
                    >
                      <View style={styles.grow}>
                        <Strong>{e.title ?? e.kind}</Strong>
                        {e.message ? <Muted>{e.message}</Muted> : null}
                      </View>
                      <Text style={[typeScale.caption, { color: t.mutedForeground }]}>
                        {ago(e.occurred_at, now)}
                      </Text>
                    </View>
                  ))}
                </Card>
              </View>
            ) : null}
          </>
        )}
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  page: { padding: space.lg, gap: space.md, paddingBottom: space.xxl * 2 },
  section: { gap: space.sm },
  eyebrow: { fontWeight: "600", letterSpacing: 0.8 },
  metaRow: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: space.md,
  },
  // `tabular-nums`: a countdown that reflows every second is a countdown nobody can read.
  mono: { fontVariant: ["tabular-nums"], fontWeight: "600" },
  stats: { flexDirection: "row", justifyContent: "space-between", gap: space.md },
  grow: { flex: 1, gap: 2 },
  event: { flexDirection: "row", alignItems: "flex-start", gap: space.md, paddingVertical: space.sm },
});
