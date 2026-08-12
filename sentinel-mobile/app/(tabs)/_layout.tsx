// The shell.
//
// FIVE tabs, in the order someone needs them: the dashboard, one-tap runs, live cameras, the
// footage those cameras kept, and settings. Capture and Edge were benches — a face-capture rig and
// a local-edge probe — and putting them on the tab bar meant handing a customer a workbench as the
// front door. They stay as ROUTES (`href: null` keeps them off the bar), reachable by us, invisible
// to the person using the app.
//
// Icons are real icons. The bar was rendering the default placeholder triangles, which is the first
// thing anyone sees and the last thing that should look unfinished.
import { Tabs } from "expo-router";
import { StyleSheet } from "react-native";
import Ionicons from "@expo/vector-icons/Ionicons";
import { useTheme } from "@/styles/theme";

export default function TabsLayout() {
  const t = useTheme();
  return (
    <Tabs
      screenOptions={{
        tabBarActiveTintColor: t.foreground,
        tabBarInactiveTintColor: t.mutedForeground,
        tabBarStyle: {
          backgroundColor: t.card,
          borderTopColor: t.border,
          borderTopWidth: StyleSheet.hairlineWidth,
        },
        tabBarLabelStyle: { fontSize: 11, fontWeight: "600" },
        headerStyle: { backgroundColor: t.card },
        headerTitleStyle: { color: t.foreground },
        headerShadowVisible: false,
      }}
    >
      <Tabs.Screen
        name="index"
        options={{
          title: "Home",
          headerShown: false,
          tabBarIcon: ({ color, size }) => <Ionicons name="home" size={size} color={color} />,
        }}
      />
      <Tabs.Screen
        name="run"
        options={{
          title: "Run",
          headerShown: false,
          tabBarIcon: ({ color, size }) => (
            <Ionicons name="play-circle" size={size} color={color} />
          ),
        }}
      />
      <Tabs.Screen
        name="live"
        options={{
          title: "Live",
          tabBarIcon: ({ color, size }) => <Ionicons name="videocam" size={size} color={color} />,
        }}
      />

      <Tabs.Screen
        name="recordings"
        options={{
          title: "Playback",
          headerShown: false,
          tabBarIcon: ({ color, size }) => <Ionicons name="film" size={size} color={color} />,
        }}
      />
      <Tabs.Screen
        name="settings"
        options={{
          title: "Settings",
          headerShown: false,
          tabBarIcon: ({ color, size }) => <Ionicons name="settings" size={size} color={color} />,
        }}
      />

      {/* Ours, not the customer's. */}
      <Tabs.Screen name="capture" options={{ href: null }} />
      <Tabs.Screen name="edge" options={{ href: null }} />
    </Tabs>
  );
}
