// Alerts — the Phase 1 payload. It answers the only question that matters right now: *is this phone
// wired up to receive alerts?* It shows the Recipient label the workflow editor will list, lets you
// fire a test push, and logs notifications as they arrive so delivery is visible in-app.
import { useEffect, useState } from "react";
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, View } from "react-native";
import * as Notifications from "expo-notifications";
import { useRouter } from "expo-router";
import { registerThisDevice, sendTestPush } from "@/push";
import { signOut } from "@/session";
import { Body, Button, Card, Danger, Heading, Muted, Screen, Status, Strong, Title } from "@/styles/ui";
import { radius, space, useTheme } from "@/styles/theme";

type Received = { id: string; title: string; body: string; at: string };

export default function Alerts() {
  const [label, setLabel] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [note, setNote] = useState<string | null>(null);
  const [received, setReceived] = useState<Received[]>([]);
  const router = useRouter();
  const t = useTheme();

  useEffect(() => {
    registerThisDevice()
      .then((r) => (r.ok ? setLabel(r.label) : setProblem(r.reason)))
      .finally(() => setBusy(false));

    const sub = Notifications.addNotificationReceivedListener((n) => {
      const c = n.request.content;
      setReceived((prev) => [
        { id: n.request.identifier, title: c.title ?? "Olhar AI", body: c.body ?? "", at: new Date().toLocaleTimeString() },
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
    <Screen>
      <ScrollView contentContainerStyle={{ padding: space.xl, gap: space.md, paddingBottom: space.xxl * 2 }}>
        <Title>Alerts</Title>

        <Card>
          {busy ? (
            <View style={{ flexDirection: "row", alignItems: "center", gap: space.md }}>
              <ActivityIndicator color={t.mutedForeground} />
              <Muted>Registering this phone…</Muted>
            </View>
          ) : label ? (
            <>
              <Status ok>Receiving alerts</Status>
              <Muted>
                This phone is the Recipient <Strong>{label}</Strong>. Pick it in a workflow's Notify Recipient
                node to send here.
              </Muted>
              <Button label="Send a test notification" onPress={test} />
              {note ? <Muted>{note}</Muted> : null}
            </>
          ) : (
            <>
              <Status ok={false}>Not receiving alerts</Status>
              <Danger>{problem}</Danger>
            </>
          )}
        </Card>

        <Heading>Received in this session</Heading>
        {received.length === 0 ? (
          <Muted>Nothing yet. Notifications that arrive while the app is open are listed here.</Muted>
        ) : (
          received.map((r) => (
            <View
              key={r.id + r.at}
              style={{
                backgroundColor: t.muted,
                borderColor: t.border,
                borderWidth: StyleSheet.hairlineWidth,
                borderRadius: radius.lg,
                padding: space.md,
                gap: 2,
              }}
            >
              <Strong>{r.title}</Strong>
              <Body>{r.body}</Body>
              <Muted>{r.at}</Muted>
            </View>
          ))
        )}

        <Pressable onPress={leave} style={{ marginTop: space.xxl, alignItems: "center", padding: space.md }}>
          <Muted>Sign out</Muted>
        </Pressable>
      </ScrollView>
    </Screen>
  );
}
