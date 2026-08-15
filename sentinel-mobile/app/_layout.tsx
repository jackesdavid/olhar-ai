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
import * as Notifications from "expo-notifications";
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
    // `approve` is reachable while signed in without being a tab — it is summoned by a push, not
    // navigated to. Excluding it from the gate keeps a tapped approval from bouncing to the tabs.
    const allowed = inApp || segments[0] === "approve" || segments[0] === "run";
    if (!signedIn && allowed) router.replace("/login");
    if (signedIn && !allowed) router.replace("/(tabs)");
  }, [ready, signedIn, segments]);

  // `olharai://run/<graph_id>` is handled by `app/run/[id].tsx`, NOT by a listener here.
  // expo-router owns incoming URLs — it maps them onto the file tree and shows "Unmatched Route"
  // for anything it cannot resolve, so a `Linking` handler never sees the widget's tap. Worse, a
  // handler that DID fire alongside the route would run the workflow twice for one press.

  // A tapped Device Gate push opens the decision, carrying the request id and the single-use token
  // that proves WHICH request is being answered (card #176). Handles both the cold start (the app
  // was not running) and a tap while it is open.
  useEffect(() => {
    if (!ready || !signedIn) return;
    function openFrom(data: Record<string, unknown> | undefined) {
      if (!data || data.kind !== "device_gate") return;
      const request_id = String(data.request_id ?? "");
      const token = String(data.token ?? "");
      if (!request_id || !token) return;
      router.push({ pathname: "/approve", params: { request_id, token } });
    }
    Notifications.getLastNotificationResponseAsync().then((r) =>
      openFrom(r?.notification.request.content.data as Record<string, unknown> | undefined),
    );
    const tapped = Notifications.addNotificationResponseReceivedListener((r) =>
      openFrom(r.notification.request.content.data as Record<string, unknown> | undefined),
    );
    // Arriving while the app is OPEN goes straight to the decision, without waiting for a tap.
    // That is what makes the widget journey feel like one gesture: tap the widget, the app opens,
    // and a second later the approval is in front of you. The notification still carries the token,
    // so nothing about the two-factor shape changes — only who does the tapping.
    const arrived = Notifications.addNotificationReceivedListener((n) =>
      openFrom(n.request.content.data as Record<string, unknown> | undefined),
    );
    return () => {
      tapped.remove();
      arrived.remove();
    };
  }, [ready, signedIn]);

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
        {/* A FULL SCREEN, not a modal sheet (#176 follow-up).
            `presentation: "modal"` is a sheet the user can drag down to dismiss, and that vertical
            gesture competes with the horizontal slide-to-approve — reported as the slider fighting
            the finger even after the ScrollView underneath it was removed. A gate is also not a
            glance: it is the only thing that matters while it is on screen, and it should own the
            screen. `gestureEnabled: false` removes the swipe-back for the same reason — the way
            out of a gate is to answer it or close it deliberately. */}
        <Stack.Screen
          name="approve"
          options={{
            headerShown: true,
            title: "Approval",
            presentation: "fullScreenModal",
            gestureEnabled: false,
          }}
        />
        <Stack.Screen name="run/[id]" options={{ headerShown: false }} />
      </Stack>
    </>
  );
}
