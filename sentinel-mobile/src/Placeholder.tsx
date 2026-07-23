// A tab that isn't built yet should say so plainly — an empty screen reads as a bug.
import { StyleSheet, Text, View } from "react-native";

export function Placeholder({ title, phase, body }: { title: string; phase: string; body: string }) {
  return (
    <View style={s.root}>
      <Text style={s.h1}>{title}</Text>
      <Text style={s.tag}>{phase}</Text>
      <Text style={s.body}>{body}</Text>
    </View>
  );
}

const s = StyleSheet.create({
  root: { flex: 1, padding: 24, justifyContent: "center", gap: 10 },
  h1: { fontSize: 30, fontWeight: "600", letterSpacing: -0.5 },
  tag: { alignSelf: "flex-start", backgroundColor: "#f0f0f0", borderRadius: 999, paddingHorizontal: 10, paddingVertical: 4, color: "#666", fontSize: 12 },
  body: { color: "#666", lineHeight: 21 },
});
