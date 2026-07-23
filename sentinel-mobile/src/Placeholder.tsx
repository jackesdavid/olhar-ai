// A tab that isn't built yet should say so plainly — an empty screen reads as a bug.
import { Text, View } from "react-native";
import { Muted, Screen, Title } from "./styles/ui";
import { radius, space, useTheme } from "./styles/theme";

export function Placeholder({ title, phase, body }: { title: string; phase: string; body: string }) {
  const t = useTheme();
  return (
    <Screen style={{ padding: space.xl, justifyContent: "center", gap: space.md }}>
      <Title>{title}</Title>
      <View
        style={{
          alignSelf: "flex-start",
          backgroundColor: t.muted,
          borderRadius: radius.pill,
          paddingHorizontal: space.md,
          paddingVertical: space.xs,
        }}
      >
        <Text style={{ color: t.mutedForeground, fontSize: 12 }}>{phase}</Text>
      </View>
      <Muted>{body}</Muted>
    </Screen>
  );
}
