// Settings — everything you touch twice a year.
//
// These used to sit on the home screen, above the things people open the app for daily. A control
// you configure once should be findable, not prominent: this tab exists so the dashboard can be
// about what is happening rather than about how the phone was set up.
import { useEffect, useState } from "react";
import { ScrollView, StyleSheet, Text, View } from "react-native";
import { useRouter } from "expo-router";
import { registerThisDevice, sendTestPush } from "@/push";
import { deleteAccount, signOut } from "@/session";
import { Body, Button, Card, Danger, Input, Loader, Muted, Screen, Status, Strong, ThemePicker } from "@/styles/ui";
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

  // Deleting an account is destructive and irreversible, so it is two deliberate steps rather than
  // one button: reveal what will happen, then confirm with the password. Apple requires the
  // capability (5.1.1(v)); saying plainly what survives is what makes it honest.
  const [confirming, setConfirming] = useState(false);
  const [password, setPassword] = useState("");
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  async function reallyDelete() {
    setDeleting(true);
    setDeleteError(null);
    try {
      await deleteAccount(password || undefined);
      router.replace("/login");
    } catch (e) {
      setDeleteError((e as Error).message);
    } finally {
      setDeleting(false);
    }
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

            <View style={styles.section}>
              <Text style={[typeScale.caption, styles.eyebrow, { color: t.mutedForeground }]}>
                DELETE ACCOUNT
              </Text>
              <Card>
                {confirming ? (
                  <>
                    {/* Named precisely, because the difference is the whole point: this removes a
                        PERSON, not a workspace. Someone deleting their login should not fear they
                        are deleting the cameras their household depends on. */}
                    <Body>This deletes your sign-in and removes you from the workspace.</Body>
                    <Muted>
                      The workspace itself carries on: cameras, automations, recordings and everyone
                      else are untouched. This phone stops receiving notifications. What you did
                      stays in the activity history.
                    </Muted>
                    <Muted>This cannot be undone.</Muted>
                    <Input
                      placeholder="Your password"
                      secureTextEntry
                      value={password}
                      onChangeText={setPassword}
                    />
                    {deleteError ? <Danger>{deleteError}</Danger> : null}
                    <Button label="Delete my account" onPress={reallyDelete} busy={deleting} />
                    <Button label="Cancel" onPress={() => { setConfirming(false); setDeleteError(null); }} />
                  </>
                ) : (
                  <>
                    <Muted>Removes you from the workspace. The workspace itself keeps running.</Muted>
                    <Button label="Delete my account" onPress={() => setConfirming(true)} />
                  </>
                )}
              </Card>
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
