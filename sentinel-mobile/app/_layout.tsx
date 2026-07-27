// Root layout + the session gate. A stored token means straight to the tabs; otherwise login.
//
// The gate MUST read the token from the session store rather than a snapshot in component state:
// signing in updates the store, and a gate holding a stale `false` redirects the freshly signed-in
// user straight back to the login screen.
import { useEffect, useState, useSyncExternalStore } from "react";
import { ActivityIndicator, View } from "react-native";
import { useLoadThemePref, useTheme } from "@/styles/theme";
import { Stack, useRouter, useSegments } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { currentToken, restore, subscribe } from "@/session";

export default function RootLayout() {
  const [ready, setReady] = useState(false);
  const signedIn = !!useSyncExternalStore(subscribe, currentToken);
  const t = useTheme();
  useLoadThemePref();
  const segments = useSegments();
  const router = useRouter();

  useEffect(() => {
    restore().finally(() => setReady(true));
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
        <Stack.Screen name="capture-settings" options={{ headerShown: true, title: "Capture settings", presentation: "modal" }} />
      </Stack>
    </>
  );
}
