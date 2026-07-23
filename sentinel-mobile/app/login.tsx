// Sign-in. On success we immediately register this phone for push, because that registration IS
// the point of Phase 1 — an operator who logs in and never opens Settings should still get alerts.
// A push failure does NOT block sign-in; it is reported on the Alerts tab instead.
import { useState } from "react";
import { ActivityIndicator, KeyboardAvoidingView, Platform, Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import { useRouter } from "expo-router";
import { signIn } from "@/session";
import { registerThisDevice } from "@/push";

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
    <KeyboardAvoidingView style={s.root} behavior={Platform.OS === "ios" ? "padding" : undefined}>
      <View style={s.card}>
        <Text style={s.brand}>Sentinel</Text>
        <Text style={s.sub}>Sign in to receive alerts on this phone.</Text>

        <TextInput
          style={s.input}
          placeholder="Email"
          autoCapitalize="none"
          autoComplete="email"
          keyboardType="email-address"
          value={email}
          onChangeText={setEmail}
        />
        <TextInput
          style={s.input}
          placeholder="Password"
          secureTextEntry
          value={password}
          onChangeText={setPassword}
          onSubmitEditing={submit}
        />

        {error ? <Text style={s.error}>{error}</Text> : null}

        <Pressable style={[s.button, busy && s.buttonBusy]} onPress={submit} disabled={busy}>
          {busy ? <ActivityIndicator color="#fff" /> : <Text style={s.buttonText}>Sign in</Text>}
        </Pressable>
      </View>
    </KeyboardAvoidingView>
  );
}

const s = StyleSheet.create({
  root: { flex: 1, justifyContent: "center", padding: 24, backgroundColor: "#fff" },
  card: { gap: 12 },
  brand: { fontSize: 34, fontWeight: "600", letterSpacing: -0.5 },
  sub: { fontSize: 15, color: "#666", marginBottom: 12 },
  input: { borderWidth: 1, borderColor: "#ddd", borderRadius: 10, padding: 14, fontSize: 16 },
  error: { color: "#b00020", fontSize: 14 },
  button: { backgroundColor: "#111", borderRadius: 10, padding: 16, alignItems: "center", marginTop: 8 },
  buttonBusy: { opacity: 0.7 },
  buttonText: { color: "#fff", fontSize: 16, fontWeight: "600" },
});
