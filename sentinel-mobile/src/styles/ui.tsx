// The shared primitives every screen builds from, so a screen never reaches for a colour literal.
// Deliberately small — these mirror the web's card / button / muted-text roles, not a component kit.
import { ActivityIndicator, Pressable, StyleSheet, Text, TextInput, View, type TextInputProps, type ViewProps } from "react-native";
import { radius, space, typeScale, useTheme } from "./theme";

export function Card({ style, ...rest }: ViewProps) {
  const t = useTheme();
  return (
    <View
      {...rest}
      style={[
        { backgroundColor: t.card, borderColor: t.border, borderWidth: StyleSheet.hairlineWidth, borderRadius: radius.xl, padding: space.lg, gap: space.md },
        style,
      ]}
    />
  );
}

export function Title({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.display, { color: t.foreground }]}>{children}</Text>;
}

export function Heading({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.title, { color: t.foreground }]}>{children}</Text>;
}

export function Body({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.body, { color: t.foreground }]}>{children}</Text>;
}

export function Muted({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.body, { color: t.mutedForeground }]}>{children}</Text>;
}

/** Failure text — the one place the ink theme allows a hue, same as the web's `--destructive`. */
export function Danger({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.body, { color: t.destructive }]}>{children}</Text>;
}

export function Strong({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={{ fontWeight: "600", color: t.foreground }}>{children}</Text>;
}

/** Status dot + label, matching the web's live/offline treatment. */
export function Status({ ok, children }: { ok: boolean; children: React.ReactNode }) {
  const t = useTheme();
  return (
    <Text style={{ fontWeight: "600", color: ok ? t.foreground : t.destructive }}>
      {ok ? "● " : "● "}
      {children}
    </Text>
  );
}

export function Input(props: TextInputProps) {
  const t = useTheme();
  return (
    <TextInput
      placeholderTextColor={t.mutedForeground}
      {...props}
      style={[
        // Inputs SINK below their panel (elev-1), same as the web.
        { backgroundColor: t.inputBg, borderColor: t.input, borderWidth: StyleSheet.hairlineWidth, borderRadius: radius.lg, padding: space.md + 2, fontSize: 16, color: t.foreground },
        props.style,
      ]}
    />
  );
}

export function Button({ label, onPress, busy, disabled }: { label: string; onPress: () => void; busy?: boolean; disabled?: boolean }) {
  const t = useTheme();
  const off = busy || disabled;
  return (
    <Pressable
      onPress={onPress}
      disabled={off}
      style={({ pressed }) => [
        { backgroundColor: t.primary, borderRadius: radius.lg, padding: space.lg, alignItems: "center" },
        off && { opacity: 0.6 },
        // `.press` on the web — tactile scale feedback.
        pressed && { transform: [{ scale: 0.97 }] },
      ]}
    >
      {busy ? (
        <ActivityIndicator color={t.primaryForeground} />
      ) : (
        <Text style={{ color: t.primaryForeground, fontSize: 16, fontWeight: "600" }}>{label}</Text>
      )}
    </Pressable>
  );
}

/** Page canvas — the base elevation stop, so cards read as panels lifted off it. */
export function Screen({ style, ...rest }: ViewProps) {
  const t = useTheme();
  return <View {...rest} style={[{ flex: 1, backgroundColor: t.background }, style]} />;
}
