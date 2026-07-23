// The shell: Alerts is Phase 1; Live (Phase 2) and Recordings (Phase 3) are placeholders that say
// so, rather than empty screens that read as broken.
import { Tabs } from "expo-router";

export default function TabsLayout() {
  return (
    <Tabs screenOptions={{ tabBarActiveTintColor: "#111" }}>
      <Tabs.Screen name="index" options={{ title: "Alerts" }} />
      <Tabs.Screen name="live" options={{ title: "Live" }} />
      <Tabs.Screen name="recordings" options={{ title: "Recordings" }} />
    </Tabs>
  );
}
