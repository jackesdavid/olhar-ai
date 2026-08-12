// Settings — everything you touch twice a year.
//
// These used to sit on the home screen, above the things people open the app for daily. A control
// you configure once should be findable, not prominent: this tab exists so the dashboard can be
// about what is happening rather than about how the phone was set up.
import { useEffect, useState } from "react";
import { ScrollView, StyleSheet, Text, View } from "react-native";
import { useRouter } from "expo-router";
import { registerThisDevice, sendTestPush } from "@/push";
import { signOut } from "@/session";
import { Button, Card, Danger, Loader, Muted, Screen, Status, Strong, ThemePicker } from "@/styles/ui";
import { space, typeScale, useTheme } from "@/styles/theme";

export default function Settings() {
  const [label, setLabel] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [note, setNote] = useState<string | null>(null);
  const router = useRouter();
  const t = useTheme();

  useEffect(() => {
    registerThisDevice()
      .then((r) => (r.ok ? setLabel(r.label) : setProblem(r.reason)))
      .finally(() => setBusy(false));
  }, []);

  async function test() {
    if (!label) return;
    setNote(null);
    try {
      const sent = await sendTestPush(label);
      setNote(sent > 0 ? "Test sent — it should arrive in a moment." : "Nothing sent — no enabled device matched.");
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
      <ScrollView contentContainerStyle={styles.page}>
        {busy ? (
          <Loader label="Checking this phone…" />
        ) : (
          <>
            <View style={styles.section}>
              <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                THIS PHONE
              </Text>
              <Card>
                {label ? (
                  <>
                    <Status ok>Receiving notifications</Status>
                    <Muted>
                      Workflows address this phone as <Strong>{label}</Strong>. Rename it under
                      Recipients in the web app — every workflow that used the old name follows.
                    </Muted>
                    <Button label="Send a test notification" onPress={test} />
                  </>
                ) : (
                  <>
                    <Status ok={false}>Not receiving notifications</Status>
                    <Danger>{problem}</Danger>
                  </>
                )}
                {note ? <Muted>{note}</Muted> : null}
              </Card>
            </View>

            <View style={styles.section}>
              <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                APPEARANCE
              </Text>
              <Card>
                <ThemePicker />
                <Muted>System follows your phone&apos;s light or dark setting.</Muted>
              </Card>
            </View>

            <View style={styles.section}>
              <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                ACCOUNT
              </Text>
              <Button label="Sign out" onPress={leave} />
            </View>
          </>
        )}
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  page: { padding: space.lg, gap: space.lg, paddingBottom: space.xxl * 2 },
  section: { gap: space.sm },
  eyebrow: { fontWeight: "600", letterSpacing: 0.8 },
});
