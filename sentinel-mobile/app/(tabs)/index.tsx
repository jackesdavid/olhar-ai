// Alerts — the Phase 1 payload. It answers the only question that matters right now: *is this phone
// wired up to receive Sentinel alerts?* It shows the Recipient label the workflow editor will list,
// lets you fire a test push, and logs notifications as they arrive so delivery is visible in-app.
import { useEffect, useState } from "react";
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import * as Notifications from "expo-notifications";
import { useRouter } from "expo-router";
import { registerThisDevice, sendTestPush } from "@/push";
import { signOut } from "@/session";

type Received = { id: string; title: string; body: string; at: string };

export default function Alerts() {
  const [label, setLabel] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [note, setNote] = useState<string | null>(null);
  const [received, setReceived] = useState<Received[]>([]);
  const router = useRouter();

  useEffect(() => {
    registerThisDevice()
      .then((r) => (r.ok ? setLabel(r.label) : setProblem(r.reason)))
      .finally(() => setBusy(false));

    const sub = Notifications.addNotificationReceivedListener((n) => {
      const c = n.request.content;
      setReceived((prev) => [
        {
          id: n.request.identifier,
          title: c.title ?? "Sentinel",
          body: c.body ?? "",
          at: new Date().toLocaleTimeString(),
        },
        ...prev,
      ]);
    });
    return () => sub.remove();
  }, []);

  async function test() {
    if (!label) return;
    setNote(null);
    try {
      const sent = await sendTestPush(label);
      setNote(sent > 0 ? `Test sent to ${sent} device(s).` : "Nothing sent — no enabled device matched.");
    } catch (e) {
      setNote((e as Error).message);
    }
  }

  async function leave() {
    await signOut();
    router.replace("/login");
  }

  return (
    <ScrollView contentContainerStyle={s.root}>
      <Text style={s.h1}>Alerts</Text>

      <View style={s.card}>
        {busy ? (
          <View style={s.row}>
            <ActivityIndicator />
            <Text style={s.muted}>Registering this phone…</Text>
          </View>
        ) : label ? (
          <>
            <Text style={s.ok}>● Receiving alerts</Text>
            <Text style={s.muted}>
              This phone is the Recipient <Text style={s.strong}>{label}</Text>. Pick it in a workflow's
              Notify Recipient node to send here.
            </Text>
            <Pressable style={s.button} onPress={test}>
              <Text style={s.buttonText}>Send a test notification</Text>
            </Pressable>
            {note ? <Text style={s.muted}>{note}</Text> : null}
          </>
        ) : (
          <>
            <Text style={s.bad}>● Not receiving alerts</Text>
            <Text style={s.muted}>{problem}</Text>
          </>
        )}
      </View>

      <Text style={s.h2}>Received in this session</Text>
      {received.length === 0 ? (
        <Text style={s.muted}>
          Nothing yet. Notifications that arrive while the app is open are listed here.
        </Text>
      ) : (
        received.map((r) => (
          <View key={r.id + r.at} style={s.item}>
            <Text style={s.strong}>{r.title}</Text>
            <Text>{r.body}</Text>
            <Text style={s.stamp}>{r.at}</Text>
          </View>
        ))
      )}

      <Pressable style={s.signout} onPress={leave}>
        <Text style={s.signoutText}>Sign out</Text>
      </Pressable>
    </ScrollView>
  );
}

const s = StyleSheet.create({
  root: { padding: 20, gap: 12, paddingBottom: 48 },
  h1: { fontSize: 30, fontWeight: "600", letterSpacing: -0.5 },
  h2: { fontSize: 17, fontWeight: "600", marginTop: 16 },
  card: { borderWidth: 1, borderColor: "#e5e5e5", borderRadius: 12, padding: 16, gap: 10 },
  row: { flexDirection: "row", alignItems: "center", gap: 10 },
  ok: { color: "#0a7d3f", fontWeight: "600" },
  bad: { color: "#b00020", fontWeight: "600" },
  muted: { color: "#666", lineHeight: 20 },
  strong: { fontWeight: "600", color: "#111" },
  button: { backgroundColor: "#111", borderRadius: 10, padding: 14, alignItems: "center" },
  buttonText: { color: "#fff", fontWeight: "600" },
  item: { borderWidth: 1, borderColor: "#eee", borderRadius: 10, padding: 12, gap: 2 },
  stamp: { color: "#999", fontSize: 12 },
  signout: { marginTop: 28, alignItems: "center", padding: 12 },
  signoutText: { color: "#666" },
});
