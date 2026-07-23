// Root layout + the session gate. A stored token means straight to the tabs; otherwise login.
import { useEffect, useState } from "react";
import { ActivityIndicator, View } from "react-native";
import { useTheme } from "@/styles/theme";
import { Stack, useRouter, useSegments } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { restore } from "@/session";

export default function RootLayout() {
  const [ready, setReady] = useState(false);
  const [signedIn, setSignedIn] = useState(false);
  const t = useTheme();
  const segments = useSegments();
  const router = useRouter();

  useEffect(() => {
    restore()
      .then((token) => setSignedIn(!!token))
      .finally(() => setReady(true));
  }, []);

  useEffect(() => {
    if (!ready) return;
    const inApp = segments[0] === "(tabs)";
    if (!signedIn && inApp) router.replace("/login");
    if (signedIn && !inApp) router.replace("/(tabs)");
  }, [ready, signedIn, segments]);

  if (!ready) {
    return (
      <View style={{ flex: 1, alignItems: "center", justifyContent: "center", backgroundColor: t.background }}>
        <ActivityIndicator color={t.mutedForeground} />
      </View>
    );
  }

  return (
    <>
      <StatusBar style="auto" />
      <Stack screenOptions={{ headerShown: false }}>
        <Stack.Screen name="login" />
        <Stack.Screen name="(tabs)" />
      </Stack>
    </>
  );
}
