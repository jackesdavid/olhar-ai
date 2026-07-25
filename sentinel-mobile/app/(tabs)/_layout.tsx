// The shell: Alerts is Phase 1; Live (Phase 2) and Recordings (Phase 3) are placeholders that say
// so, rather than empty screens that read as broken. Chrome colours come from the shared tokens so
// the tab bar matches the web app's surfaces instead of the RN defaults.
import { Tabs } from "expo-router";
import { StyleSheet } from "react-native";
import { useTheme } from "@/styles/theme";

export default function TabsLayout() {
  const t = useTheme();
  return (
    <Tabs
      screenOptions={{
        tabBarActiveTintColor: t.foreground,
        tabBarInactiveTintColor: t.mutedForeground,
        tabBarStyle: { backgroundColor: t.card, borderTopColor: t.border, borderTopWidth: StyleSheet.hairlineWidth },
        headerStyle: { backgroundColor: t.card },
        headerTitleStyle: { color: t.foreground },
        headerShadowVisible: false,
      }}
    >
      <Tabs.Screen name="index" options={{ title: "Alerts" }} />
      <Tabs.Screen name="live" options={{ title: "Live" }} />
      <Tabs.Screen name="capture" options={{ title: "Capture" }} />
      <Tabs.Screen name="recordings" options={{ title: "Recordings" }} />
    </Tabs>
  );
}
