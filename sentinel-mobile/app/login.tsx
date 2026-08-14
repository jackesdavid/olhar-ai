// Sign-in. On success we immediately register this phone for push, because that registration IS
// the point of Phase 1 — an operator who logs in and never opens Settings should still get alerts.
// A push failure does NOT block sign-in; it is reported on the Alerts tab instead.
import { useState } from "react";
import { KeyboardAvoidingView, Platform, Pressable } from "react-native";
import { useRouter } from "expo-router";
import { signIn } from "@/session";
import { registerThisDevice } from "@/push";
import { Button, Danger, Input, Muted, Screen, Title } from "@/styles/ui";
import { space } from "@/styles/theme";

export default function Login() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const router = useRouter();
  async function submit() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await signIn(email, password);
      // Best-effort: the result surfaces on the Alerts tab, which re-checks registration.
      void registerThisDevice();
      router.replace("/(tabs)");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Screen>
      <KeyboardAvoidingView
        style={{ flex: 1, justifyContent: "center", padding: space.xl, gap: space.md }}
        behavior={Platform.OS === "ios" ? "padding" : undefined}
      >
        <Title>Olhar AI</Title>
        <Muted>Sign in to receive alerts on this phone.</Muted>

        <Input
          placeholder="Email"
          autoCapitalize="none"
          autoComplete="email"
          keyboardType="email-address"
          value={email}
          onChangeText={setEmail}
        />
        <Input placeholder="Password" secureTextEntry value={password} onChangeText={setPassword} onSubmitEditing={submit} />

        {error ? <Danger>{error}</Danger> : null}

        <Button label="Sign in" onPress={submit} busy={busy} />

        {/* Someone who was invited has no account yet and no reason to guess that a scan screen
            exists. It has to be on the first screen, not behind a menu. */}
        <Pressable onPress={() => router.replace("/invite")} style={{ alignSelf: "center", paddingVertical: space.md }}>
          <Muted>I have an invite</Muted>
        </Pressable>
      </KeyboardAvoidingView>
    </Screen>
  );
}
